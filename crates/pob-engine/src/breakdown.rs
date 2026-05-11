//! Step-by-step re-derivation of a small set of headline outputs.
//!
//! Slice 4 of [#34](https://github.com/jonatanferm/PathOfBuildingMK2/issues/34) — the
//! "click a Calcs-tab row, see how the number was built" bit. Mirrors
//! `Modules/CalcBreakdown.lua` for the top-three sections (Damage, Speed,
//! Crit). Other rows fall back to the contributing-mods enumerator the
//! Calcs tab already had.
//!
//! ## Shape
//!
//! [`derive_for`] takes the live [`Env`] (post-`compute_full`) and the
//! output key the user clicked. It returns a [`Breakdown`] consisting of
//! ordered [`BreakdownStep`] entries, each describing one slice of the
//! derivation:
//!
//! ```text
//! Base            660           ^8(skill at level 20)
//!  + flat adds    +50–80        ^8(weapon adds + tree)
//! INC             +120 %        Tree, Item slot 3, ...
//! MORE            ×1.30         Item slot 2 ("More Spell Damage"), ...
//!  = 1748–1924    avg 1836
//! ```
//!
//! The UI consumes the steps in order and renders each with its label,
//! computed `value`, optional `explain` text, and the list of source
//! mods. Steps with `sources.is_empty()` show only the label/value.
//!
//! ## What's covered
//!
//! Damage:    `MainSkillAverageHit`, `MainSkillAverageHitWithCrit`,
//!            `MainSkillDPS`, `TotalDPS`, `FullDPS`, `AverageHit`, `AverageDamage`
//! Speed:     `Speed`, `MainSkillSpeed`, `AttackSpeedMult`, `CastSpeedMult`,
//!            `MovementSpeedMod`
//! Crit:      `CritChance`, `MainSkillCritChance`, `CritMultiplier`, `CritEffect`
//!
//! Output keys not in the table fall through to `derive_for` returning
//! `None`; the UI then renders the legacy contributing-mods view.

use crate::env::Env;
use crate::mod_db::{ModStore, QueryCfg};
use crate::modifier::{Mod, ModType, Source};

/// One contributing modifier surfaced inside a [`BreakdownStep`].
#[derive(Debug, Clone)]
pub struct ModSource {
    /// `(1 + value/100)` for INC/MORE, raw for BASE — same convention
    /// `Mod` carries. Optional because some sources are synthetic (e.g.
    /// the skill's intrinsic crit).
    pub value: Option<f64>,
    /// Modifier kind: BASE / INC / MORE / Override / Flag / List.
    pub kind: ModType,
    /// Provenance label, derived from `Mod::source`.
    pub source: String,
}

impl ModSource {
    fn from_mod(m: &Mod) -> Self {
        Self {
            value: m.value.as_f64(),
            kind: m.kind,
            source: source_label(m.source.as_ref()),
        }
    }
}

/// One step in a breakdown.
#[derive(Debug, Clone, Default)]
pub struct BreakdownStep {
    /// Short, human-readable label — the leftmost text on the row
    /// (e.g. `"Base"`, `"Increased"`, `"More"`, `"Crit factor"`).
    pub label: String,
    /// Numeric quantity for this step. Steps with no inherent number
    /// (purely explanatory rows) leave this `None`.
    pub value: Option<f64>,
    /// Free-form explanation that appears in dim text on the right —
    /// matches PoB's `^8(...)` annotations in `Modules/CalcBreakdown.lua`.
    pub explain: Option<String>,
    /// Mods (or synthetic sources) contributing to this step.
    pub sources: Vec<ModSource>,
}

impl BreakdownStep {
    fn label(label: impl Into<String>) -> Self {
        Self {
            label: label.into(),
            ..Self::default()
        }
    }
    fn with_value(mut self, v: f64) -> Self {
        self.value = Some(v);
        self
    }
    fn with_explain(mut self, e: impl Into<String>) -> Self {
        self.explain = Some(e.into());
        self
    }
    fn with_sources(mut self, s: Vec<ModSource>) -> Self {
        self.sources = s;
        self
    }
}

/// A full breakdown. The Calcs tab walks the steps top-down and renders
/// each as one or more rows, matching PoB's tooltip ordering.
#[derive(Debug, Clone, Default)]
pub struct Breakdown {
    /// The output key the breakdown is for.
    pub output_key: String,
    /// Final value the chain resolves to. The UI uses this to verify the
    /// breakdown matches what's currently in `env.output` (a sanity check;
    /// floating-point rounding may make them mildly differ).
    pub total: f64,
    /// Ordered derivation steps.
    pub steps: Vec<BreakdownStep>,
}

/// Public entry point. Returns `None` if `output_key` doesn't have a
/// dedicated breakdown — the UI then falls back to the legacy
/// "contributing mods" view.
#[must_use]
pub fn derive_for(env: &Env, output_key: &str) -> Option<Breakdown> {
    match output_key {
        // Damage.
        "MainSkillAverageHit" => Some(damage_average_hit(env)),
        "MainSkillAverageHitWithCrit" | "AverageHit" | "AverageDamage" => {
            Some(damage_average_hit_with_crit(env, output_key))
        }
        "MainSkillDPS" | "TotalDPS" => Some(damage_dps(env, output_key)),
        "FullDPS" => Some(damage_full_dps(env)),

        // Speed.
        "MainSkillSpeed" | "Speed" => Some(speed_main_skill(env, output_key)),
        "AttackSpeedMult" => Some(speed_simple_mult(
            env,
            "AttackSpeed",
            "AttackSpeedMult",
            "Attack speed multiplier",
        )),
        "CastSpeedMult" => Some(speed_simple_mult(
            env,
            "CastSpeed",
            "CastSpeedMult",
            "Cast speed multiplier",
        )),
        "MovementSpeedMod" => Some(speed_simple_mult(
            env,
            "MovementSpeed",
            "MovementSpeedMod",
            "Movement speed multiplier",
        )),
        // Issue #34 follow-up: AreaOfEffectMod walks through the same
        // (1 + INC%/100) × MORE shape as the speed mults, so we can
        // reuse `speed_simple_mult` against the `AreaOfEffect` mod
        // store. `perform.rs` populates `AreaOfEffectMod` directly.
        "AreaOfEffectMod" => Some(speed_simple_mult(
            env,
            "AreaOfEffect",
            "AreaOfEffectMod",
            "Area of effect modifier",
        )),
        "AreaOfEffectRadius" => area_of_effect_radius(env),
        "AreaOfEffectRadiusMetres" => area_of_effect_radius_metres(env),
        "ProjectileCount" => projectile_count(env),
        "ProjectileMultiplier" => projectile_multiplier(env),
        "MainSkillShockMult" => Some(main_skill_shock_mult(env)),
        "IgniteDuration" => ignite_duration(env),
        "PoisonDuration" => ailment_duration(
            env,
            "PoisonDuration",
            2.0,
            "2s base poison duration (PoE constant)",
            "Poison duration",
        ),
        "BleedDuration" => ailment_duration(
            env,
            "BleedDuration",
            5.0,
            "5s base bleed duration (PoE constant)",
            "Bleed duration",
        ),
        "PoisonStacks" => poison_stacks(env),
        "PoisonStackLimit" => poison_stack_limit(env),
        "BleedChance" => bleed_chance(env),
        "IgniteChance" => ignite_chance(env),
        "ShockChance" => on_hit_plus_crit_chance(
            env,
            "ShockChance",
            "ShockChanceOnHit",
            "Shock chance",
            "shock",
        ),
        "FreezeChance" => on_hit_plus_crit_chance(
            env,
            "FreezeChance",
            "FreezeChanceOnHit",
            "Freeze chance",
            "freeze",
        ),
        "ManaRegenRecovery" => mana_regen_recovery(env),
        "FireResistMax" => resist_max(env, "FireResistMax", "Fire resistance maximum"),
        "ColdResistMax" => resist_max(env, "ColdResistMax", "Cold resistance maximum"),
        "LightningResistMax" => {
            resist_max(env, "LightningResistMax", "Lightning resistance maximum")
        }
        "ChaosResistMax" => resist_max(env, "ChaosResistMax", "Chaos resistance maximum"),
        // Issue #34 follow-up: per-element TotalHitPool / TotalPool
        // outputs all collapse to the same `life + es + ward` pool
        // (Phase 2 — no MoM yet), so dispatch all 10 keys through
        // the same shared helper.
        "PhysicalTotalHitPool" => {
            total_pool(env, "Physical", "TotalHitPool", "Physical total hit pool")
        }
        "FireTotalHitPool" => total_pool(env, "Fire", "TotalHitPool", "Fire total hit pool"),
        "ColdTotalHitPool" => total_pool(env, "Cold", "TotalHitPool", "Cold total hit pool"),
        "LightningTotalHitPool" => {
            total_pool(env, "Lightning", "TotalHitPool", "Lightning total hit pool")
        }
        "ChaosTotalHitPool" => total_pool(env, "Chaos", "TotalHitPool", "Chaos total hit pool"),
        "PhysicalTotalPool" => total_pool(env, "Physical", "TotalPool", "Physical total pool"),
        "FireTotalPool" => total_pool(env, "Fire", "TotalPool", "Fire total pool"),
        "ColdTotalPool" => total_pool(env, "Cold", "TotalPool", "Cold total pool"),
        "LightningTotalPool" => total_pool(env, "Lightning", "TotalPool", "Lightning total pool"),
        "ChaosTotalPool" => total_pool(env, "Chaos", "TotalPool", "Chaos total pool"),
        // Issue #34 follow-up: per-element MoMHitPool /
        // ManaEffectiveLife outputs share the same pool sum (Phase
        // 2 baseline). Plus the shared (non-element) variants.
        "PhysicalMoMHitPool" => total_pool(env, "Physical", "MoMHitPool", "Physical MoM hit pool"),
        "FireMoMHitPool" => total_pool(env, "Fire", "MoMHitPool", "Fire MoM hit pool"),
        "ColdMoMHitPool" => total_pool(env, "Cold", "MoMHitPool", "Cold MoM hit pool"),
        "LightningMoMHitPool" => {
            total_pool(env, "Lightning", "MoMHitPool", "Lightning MoM hit pool")
        }
        "ChaosMoMHitPool" => total_pool(env, "Chaos", "MoMHitPool", "Chaos MoM hit pool"),
        "PhysicalManaEffectiveLife" => total_pool(
            env,
            "Physical",
            "ManaEffectiveLife",
            "Physical mana-effective life",
        ),
        "FireManaEffectiveLife" => {
            total_pool(env, "Fire", "ManaEffectiveLife", "Fire mana-effective life")
        }
        "ColdManaEffectiveLife" => {
            total_pool(env, "Cold", "ManaEffectiveLife", "Cold mana-effective life")
        }
        "LightningManaEffectiveLife" => total_pool(
            env,
            "Lightning",
            "ManaEffectiveLife",
            "Lightning mana-effective life",
        ),
        "ChaosManaEffectiveLife" => total_pool(
            env,
            "Chaos",
            "ManaEffectiveLife",
            "Chaos mana-effective life",
        ),
        "sharedMoMHitPool" => total_pool(env, "shared", "MoMHitPool", "Shared MoM hit pool"),
        "sharedManaEffectiveLife" => total_pool(
            env,
            "shared",
            "ManaEffectiveLife",
            "Shared mana-effective life",
        ),
        // Issue #34 follow-up: Phase 2 baseline aliases for Life
        // (no recoup, no MoM split). All three rows collapse to the
        // Life output; the breakdown calls out the alias.
        "LifeHitPool" => life_alias(env, "LifeHitPool", "Life hit pool"),
        "LifeRecoverable" => life_alias(env, "LifeRecoverable", "Life recoverable"),
        "StunThreshold" => life_alias(env, "StunThreshold", "Stun threshold"),
        "PhysicalDotEHP" => dot_ehp(env, "Physical"),
        "FireDotEHP" => dot_ehp(env, "Fire"),
        "ColdDotEHP" => dot_ehp(env, "Cold"),
        "LightningDotEHP" => dot_ehp(env, "Lightning"),
        "ChaosDotEHP" => dot_ehp(env, "Chaos"),
        "PhysicalMaximumHitTaken" => maximum_hit_taken(env, "Physical"),
        "SecondMinimalMaximumHitTaken" => second_min_max_hit_taken(env),
        "MainSkillHitMin" => main_skill_hit_bound(env, "Min"),
        "MainSkillHitMax" => main_skill_hit_bound(env, "Max"),
        "MainSkillBaseMin" => main_skill_base_bound(env, "Min"),
        "MainSkillBaseMax" => main_skill_base_bound(env, "Max"),
        "FireResist" => uncapped_elemental_resist(env, "Fire"),
        "ColdResist" => uncapped_elemental_resist(env, "Cold"),
        "LightningResist" => uncapped_elemental_resist(env, "Lightning"),
        "ChaosResist" => uncapped_chaos_resist(env),
        "WeaponRangeMetre" => weapon_range_metre(env),
        "MainSkillLevel" => main_skill_level(env),
        "CastRate" => cast_rate(env),
        "EnemyPhysReduction" => enemy_phys_reduction(env),
        "MainSkillEnemyEffectiveResist" => main_skill_enemy_effective_resist(env),
        "LifeFlaskRecovery" => flask_recovery(env, "Life"),
        "ManaFlaskRecovery" => flask_recovery(env, "Mana"),
        "AoERadius" => aoe_radius_base(env),
        "LifeReservedPercent" => reservation_percent(env, "Life", "Reserved"),
        "ManaReservedPercent" => reservation_percent(env, "Mana", "Reserved"),
        "LifeUnreservedPercent" => reservation_percent(env, "Life", "Unreserved"),
        "ManaUnreservedPercent" => reservation_percent(env, "Mana", "Unreserved"),
        "LifeReserved" => reserved_pool(env, "Life"),
        "ManaReserved" => reserved_pool(env, "Mana"),
        "FireHitAverage" => element_hit_average(env, "Fire"),
        "ColdHitAverage" => element_hit_average(env, "Cold"),
        "LightningHitAverage" => element_hit_average(env, "Lightning"),
        "PhysicalHitAverage" => element_hit_average(env, "Physical"),
        "ChaosHitAverage" => element_hit_average(env, "Chaos"),
        // Issue #34 follow-up: per-element Min / Max / MinBase /
        // MaxBase outputs share the same shape as MainSkillHit /
        // MainSkillBase{Min,Max} but per damage type. Twenty keys
        // total via two shared helpers.
        "FireMin" => element_hit_bound(env, "Fire", "Min"),
        "FireMax" => element_hit_bound(env, "Fire", "Max"),
        "ColdMin" => element_hit_bound(env, "Cold", "Min"),
        "ColdMax" => element_hit_bound(env, "Cold", "Max"),
        "LightningMin" => element_hit_bound(env, "Lightning", "Min"),
        "LightningMax" => element_hit_bound(env, "Lightning", "Max"),
        "PhysicalMin" => element_hit_bound(env, "Physical", "Min"),
        "PhysicalMax" => element_hit_bound(env, "Physical", "Max"),
        "ChaosMin" => element_hit_bound(env, "Chaos", "Min"),
        "ChaosMax" => element_hit_bound(env, "Chaos", "Max"),
        "FireMinBase" => element_base_bound(env, "Fire", "Min"),
        "FireMaxBase" => element_base_bound(env, "Fire", "Max"),
        "ColdMinBase" => element_base_bound(env, "Cold", "Min"),
        "ColdMaxBase" => element_base_bound(env, "Cold", "Max"),
        "LightningMinBase" => element_base_bound(env, "Lightning", "Min"),
        "LightningMaxBase" => element_base_bound(env, "Lightning", "Max"),
        "PhysicalMinBase" => element_base_bound(env, "Physical", "Min"),
        "PhysicalMaxBase" => element_base_bound(env, "Physical", "Max"),
        "ChaosMinBase" => element_base_bound(env, "Chaos", "Min"),
        "ChaosMaxBase" => element_base_bound(env, "Chaos", "Max"),
        "FireMaximumHitTaken" => maximum_hit_taken(env, "Fire"),
        "ColdMaximumHitTaken" => maximum_hit_taken(env, "Cold"),
        "LightningMaximumHitTaken" => maximum_hit_taken(env, "Lightning"),
        "ChaosMaximumHitTaken" => maximum_hit_taken(env, "Chaos"),
        "BlockChanceMax" => default_plus_base_max(
            env,
            "BlockChanceMax",
            "Block chance maximum",
            75.0,
            "75% default block cap (PoE constant)",
            "Glancing Blows / Bone Offering / ascendancy",
        ),
        "LifeUnreserved" => unreserved_pool(env, "Life"),
        "ManaUnreserved" => unreserved_pool(env, "Mana"),
        "TotalAttr" => Some(total_attributes(env)),
        "LowestAttribute" => Some(lowest_attribute(env)),
        "LowestOfMaximumLifeAndMaximumMana" => lowest_pool(env),
        "MaxLifeLeechRate" => max_leech_rate(env, "Life"),
        "MaxManaLeechRate" => max_leech_rate(env, "Mana"),
        "MaxLifeLeechInstance" => leech_instance(
            env,
            "Life",
            "MaxLifeLeechInstance",
            "Per-instance cap",
            "Max life leech instance",
            0.10,
            "10% of pool per instance (PoE constant)",
        ),
        "MaxManaLeechInstance" => leech_instance(
            env,
            "Mana",
            "MaxManaLeechInstance",
            "Per-instance cap",
            "Max mana leech instance",
            0.10,
            "10% of pool per instance (PoE constant)",
        ),
        "LifeLeechInstanceRate" => leech_instance(
            env,
            "Life",
            "LifeLeechInstanceRate",
            "Per-instance rate",
            "Life leech instance rate",
            0.02,
            "2% of pool per second per instance (PoE constant)",
        ),
        "ManaLeechInstanceRate" => leech_instance(
            env,
            "Mana",
            "ManaLeechInstanceRate",
            "Per-instance rate",
            "Mana leech instance rate",
            0.02,
            "2% of pool per second per instance (PoE constant)",
        ),

        // Crit.
        "CritChance" | "MainSkillCritChance" => Some(crit_chance(env, output_key)),
        "CritMultiplier" => Some(crit_multiplier(env)),
        "CritEffect" => Some(crit_effect(env)),

        // Attributes — class-start BASE plus `+N to <attr>` / `+N to all
        // attributes` mods stack into a single BASE total. INC mods on
        // attributes (rare, ascendancy / unique jewels) compose through
        // `pool_basic`'s standard chain.
        "Strength" => Some(pool_basic(env, "Strength")),
        "Dexterity" => Some(pool_basic(env, "Dexterity")),
        "Intelligence" => Some(pool_basic(env, "Intelligence")),

        // Pools.
        "Life" => Some(pool_with_attribute(env, "Life", "Strength", 2.0)),
        "Mana" => Some(pool_with_attribute(env, "Mana", "Intelligence", 2.0)),
        // Energy Shield draws from item-base intrinsics + the Int → Inc
        // EnergyShield wiring, not from a divisor-based contribution like
        // Life / Mana.
        "EnergyShield" => Some(pool_basic(env, "EnergyShield")),
        // Armour / Evasion / Ward share the same `BASE × (1+Inc/100) × MORE`
        // shape — Strength → Inc Armour and Dex → Inc Evasion show up
        // as INC mods sourced as `Other("Strength")` / `Other("Dexterity")`,
        // landing under the Increased step's source list.
        "Armour" => Some(pool_basic(env, "Armour")),
        // Defensive: armour vs a 1000-pt baseline phys hit. Surfaces
        // PoB's `armour / (armour + 12 × baseline)` formula and the
        // 90% hard cap.
        "PhysicalDamageReduction" => Some(physical_damage_reduction(env)),
        // Effective HP per damage type — pool / damage-taken-multiplier.
        // Phys folds in armour-derived reduction + block; elemental and
        // chaos share one helper that walks the resist + suppression +
        // spell-block chain, parameterised on the resist key + element
        // label + 3% pen flag (chaos uses no pen per PoB's standard
        // Pinnacle Boss preset).
        "PhysicalEHP" => Some(physical_ehp(env)),
        "FireEHP" => Some(elemental_ehp(
            env,
            "Fire",
            "FireResistTotal",
            "FireEHP",
            true,
        )),
        "ColdEHP" => Some(elemental_ehp(
            env,
            "Cold",
            "ColdResistTotal",
            "ColdEHP",
            true,
        )),
        "LightningEHP" => Some(elemental_ehp(
            env,
            "Lightning",
            "LightningResistTotal",
            "LightningEHP",
            true,
        )),
        "ChaosEHP" => Some(elemental_ehp(
            env,
            "Chaos",
            "ChaosResistTotal",
            "ChaosEHP",
            false,
        )),
        // Aggregate EHPs — average across the five elements (handy
        // baseline) and minimum (worst-case damage type the build is
        // weakest to). Both surface all five contributors so the user
        // can see what's pulling the figure.
        "AverageEHP" => Some(average_ehp(env)),
        "MinimumEHP" => Some(minimum_ehp(env)),
        // Hits-to-die — `pool / total_taken_hit` against PoB's
        // standard Pinnacle Boss preset (mixed elemental + chaos hit).
        "NumberOfDamagingHits" => Some(number_of_damaging_hits(env)),
        // Survival time in seconds against the same Pinnacle Boss
        // preset — `hits_to_die × enemySkillTime`.
        "EHPSurvivalTime" => Some(ehp_survival_time(env)),
        // Headline defensive number — folds the Pinnacle Boss's mixed
        // damage profile into a single value.
        "TotalEHP" => Some(total_ehp(env)),
        // Poison DPS: per-stack × steady-state stack count. Per-stack
        // back-derived from the stored outputs since the perform pass
        // doesn't expose it directly.
        "PoisonDPS" => Some(poison_dps(env)),
        // Bleed / Ignite DPS: single-application × chance. Both share
        // the same shape, so one helper handles both with the chance
        // output key + label distinguishing them.
        "BleedDPS" => Some(single_app_ailment_dps(
            env,
            "BleedDPS",
            "BleedChance",
            "Bleed",
        )),
        "IgniteDPS" => Some(single_app_ailment_dps(
            env,
            "IgniteDPS",
            "IgniteChance",
            "Ignite",
        )),
        // Impale: 4-knob phys-attack damage layer. Stacks (default 5)
        // are noted in the explain text rather than as their own
        // step since most builds don't tune them.
        "ImpaleDPS" => Some(impale_dps(env)),
        "Evasion" => Some(pool_basic(env, "Evasion")),
        "Ward" => Some(pool_basic(env, "Ward")),

        // Resists. Total = min(raw, cap) where raw = BASE(<elem>Resist) +
        // BASE(ElementalResist) + level-penalty, and cap = 75 +
        // BASE(<elem>ResistMax). Chaos has no umbrella adder.
        "FireResistTotal" => Some(elemental_resist(env, "Fire")),
        "ColdResistTotal" => Some(elemental_resist(env, "Cold")),
        "LightningResistTotal" => Some(elemental_resist(env, "Lightning")),
        "ChaosResistTotal" => Some(chaos_resist(env)),

        // Damage avoidance. Block / Spell Block / Spell Suppression all
        // share the same `min(Σ BASE, cap)` shape — Block at 75 + max
        // bonus, Spell Suppression at 100 (no per-mod cap raise).
        "BlockChance" => Some(capped_chance(
            env,
            "BlockChance",
            Some("BlockChanceMax"),
            "Attack Block",
            75.0,
        )),
        "SpellBlockChance" => Some(capped_chance(
            env,
            "SpellBlockChance",
            // PoB shares the BlockChanceMax cap with spell block — both
            // top out at the same percentage.
            Some("BlockChanceMax"),
            "Spell Block",
            75.0,
        )),
        "SpellSuppressionChance" => Some(capped_chance(
            env,
            "SpellSuppressionChance",
            None,
            "Spell Suppression",
            100.0,
        )),

        // Hit chance — Accuracy is the input to the formal hit-chance
        // formula; surfacing its three additive contributors (BASE
        // mods, level term, Dex term) is the most-asked breakdown for
        // attack builds tuning accuracy investment.
        "Accuracy" => Some(accuracy(env)),
        "MainSkillHitChance" => Some(hit_chance_main_skill(env)),
        "AccuracyHitChance" => accuracy_hit_chance(env),

        // Damage chain — the after-resist hit shows the multiplicative
        // step from `AverageHitWithCrit` to the post-resist value. Lets
        // the user see how much damage the enemy's resist is shaving.
        "MainSkillAverageHitAfterResist" => Some(after_resist(env)),
        // After-shock continues the chain: AfterResist × ShockMult plus
        // any `PhysicalGainAsExtraDamage` (gain-as-elemental adders).
        "MainSkillAverageHitAfterShock" => Some(after_shock(env)),
        // After-accuracy completes the chain: AfterShock × HitChance.
        // Spells always hit; attacks scale by the formal hit chance.
        "MainSkillAverageHitAfterAccuracy" => Some(after_accuracy(env)),
        // Spell-resource cost — the active skill's mana cost after
        // the player's `ManaCost` Inc/Reduce mods. Spell builds tuning
        // Lifetap / mana-efficiency want this surfaced.
        "MainSkillManaCost" => Some(main_skill_mana_cost(env)),
        "ManaPerSecondCost" => Some(mana_per_second_cost(env)),

        // With{Ailment}DPS sums — `MainSkillDPS + <ailment>DPS`. One
        // helper handles all four; the ailment label / source key
        // distinguish the rendered steps.
        "WithBleedDPS" => Some(with_ailment_dps(
            env,
            "WithBleedDPS",
            "BleedDPS",
            "Bleed DPS",
        )),
        "WithPoisonDPS" => Some(with_ailment_dps(
            env,
            "WithPoisonDPS",
            "PoisonDPS",
            "Poison DPS",
        )),
        "WithIgniteDPS" => Some(with_ailment_dps(
            env,
            "WithIgniteDPS",
            "IgniteDPS",
            "Ignite DPS",
        )),
        "WithImpaleDPS" => Some(with_ailment_dps(
            env,
            "WithImpaleDPS",
            "ImpaleDPS",
            "Impale DPS",
        )),

        // Recovery — Life regen and Mana regen. PoB exposes both with
        // their flat / percent / pool-tied compositions.
        "LifeRegen" => Some(life_regen(env)),
        "ManaRegen" => Some(mana_regen(env)),
        "EnergyShieldRegen" => energy_shield_regen(env),
        "EnergyShieldRecharge" => energy_shield_recharge(env),

        _ => None,
    }
}

/// Names of every output key that has a custom breakdown. Useful for
/// integration tests / docs.
pub const COVERED_KEYS: &[&str] = &[
    // Attributes.
    "Strength",
    "Dexterity",
    "Intelligence",
    "TotalAttr",
    "LowestAttribute",
    "LowestOfMaximumLifeAndMaximumMana",
    "MaxLifeLeechRate",
    "MaxManaLeechRate",
    "MaxLifeLeechInstance",
    "MaxManaLeechInstance",
    "LifeLeechInstanceRate",
    "ManaLeechInstanceRate",
    // Hit chance.
    "Accuracy",
    "MainSkillHitChance",
    // Damage chain.
    "MainSkillAverageHitAfterResist",
    "MainSkillAverageHitAfterShock",
    "MainSkillAverageHitAfterAccuracy",
    // Spell-resource cost.
    "MainSkillManaCost",
    "ManaPerSecondCost",
    // With-ailment DPS rollups.
    "WithBleedDPS",
    "WithPoisonDPS",
    "WithIgniteDPS",
    "WithImpaleDPS",
    // Damage.
    "MainSkillAverageHit",
    "MainSkillAverageHitWithCrit",
    "AverageHit",
    "AverageDamage",
    "MainSkillDPS",
    "TotalDPS",
    "FullDPS",
    // Speed.
    "MainSkillSpeed",
    "Speed",
    "AttackSpeedMult",
    "CastSpeedMult",
    "MovementSpeedMod",
    "AreaOfEffectMod",
    "AreaOfEffectRadius",
    "AreaOfEffectRadiusMetres",
    "ProjectileCount",
    "ProjectileMultiplier",
    "MainSkillShockMult",
    "IgniteDuration",
    "PoisonDuration",
    "BleedDuration",
    "PoisonStacks",
    "PoisonStackLimit",
    // Crit.
    "CritChance",
    "MainSkillCritChance",
    "CritMultiplier",
    "CritEffect",
    // Pools.
    "Life",
    "Mana",
    "EnergyShield",
    "Armour",
    "PhysicalDamageReduction",
    // Effective HP — physical layer first; then per-element + chaos.
    "PhysicalEHP",
    "FireEHP",
    "ColdEHP",
    "LightningEHP",
    "ChaosEHP",
    "AverageEHP",
    "MinimumEHP",
    "NumberOfDamagingHits",
    "EHPSurvivalTime",
    "TotalEHP",
    "PoisonDPS",
    "BleedDPS",
    "IgniteDPS",
    "ImpaleDPS",
    "Evasion",
    "Ward",
    // Resists.
    "FireResistTotal",
    "ColdResistTotal",
    "LightningResistTotal",
    "ChaosResistTotal",
    // Damage avoidance.
    "BlockChance",
    "SpellBlockChance",
    "SpellSuppressionChance",
    // Recovery.
    "LifeRegen",
    "ManaRegen",
    "EnergyShieldRegen",
    "LifeUnreserved",
    "ManaUnreserved",
    // EnergyShieldRecharge is intentionally absent here — the breakdown
    // returns `None` when EnergyShield = 0, and this fixture pins a
    // pure-Life build (the EHP-pool tests assert against
    // Life + Ward = 1100). Adding ES to the fixture for the COVERED_KEYS
    // guard would shift those EHP totals and break unrelated tests; the
    // recharge breakdown itself has dedicated tests below.
];

// --- Damage ---------------------------------------------------------

/// Re-derive `MainSkillAverageHit` from base-min/max, INC/MORE on
/// Damage / ElementalDamage / element-specific / SpellDamage, plus the
/// flat-quality bonus.
///
/// Mirrors `perform_skill_dps` lines 2540-2578 (commit ref).
fn damage_average_hit(env: &Env) -> Breakdown {
    let base_min = env.output.get("MainSkillBaseMin");
    let base_max = env.output.get("MainSkillBaseMax");
    let hit_min = env.output.get("MainSkillHitMin");
    let hit_max = env.output.get("MainSkillHitMax");
    let avg = env.output.get("MainSkillAverageHit");

    // Recover the `(1+inc)*more` factor from the input/output ratio.
    let mult = if base_min > 0.0 || base_max > 0.0 {
        (hit_min + hit_max) / (base_min + base_max).max(1e-9)
    } else {
        1.0
    };

    let mut steps = Vec::new();
    steps.push(
        BreakdownStep::label("Base damage")
            .with_value((base_min + base_max) * 0.5)
            .with_explain(format!("min {base_min:.0} – max {base_max:.0}")),
    );

    // INC sources: pull from a default cfg — this is best-effort surfacing
    // for the user. The actual numeric multiplier comes from the
    // hit_max / base_max ratio so it always matches the displayed value.
    let mut inc_mods = Vec::new();
    let mut inc_total = 0.0_f64;
    let damage_keys: &[&str] = &[
        "Damage",
        "ElementalDamage",
        "FireDamage",
        "ColdDamage",
        "LightningDamage",
        "PhysicalDamage",
        "ChaosDamage",
        "SpellDamage",
        "AttackDamage",
    ];
    for key in damage_keys {
        for m in env.mod_db.iter_named(key) {
            if m.kind != ModType::Inc {
                continue;
            }
            if let Some(v) = m.value.as_f64() {
                inc_total += v;
                inc_mods.push(ModSource::from_mod(m));
            }
        }
    }
    if !inc_mods.is_empty() || inc_total != 0.0 {
        steps.push(
            BreakdownStep::label("Increased")
                .with_value(1.0 + inc_total / 100.0)
                .with_explain(format!("{inc_total:+.0}% increased / reduced"))
                .with_sources(inc_mods),
        );
    }

    let mut more_mods = Vec::new();
    let mut more_factor: f64 = 1.0;
    for key in damage_keys {
        for m in env.mod_db.iter_named(key) {
            if m.kind != ModType::More {
                continue;
            }
            if let Some(v) = m.value.as_f64() {
                more_factor *= 1.0 + v / 100.0;
                more_mods.push(ModSource::from_mod(m));
            }
        }
    }
    if !more_mods.is_empty() || (more_factor - 1.0).abs() > 1e-9 {
        steps.push(
            BreakdownStep::label("More")
                .with_value(more_factor)
                .with_explain("multiplicative".to_owned())
                .with_sources(more_mods),
        );
    }

    // The (1+inc/100)*more product as a single line — useful when the
    // INC and MORE breakdowns above are noisy.
    if mult.is_finite() && (mult - 1.0).abs() > 1e-9 {
        steps.push(
            BreakdownStep::label("Effective multiplier")
                .with_value(mult)
                .with_explain(format!("(1 + {inc_total:.0}%) × {more_factor:.3}")),
        );
    }

    // Quality bonus — flat 0.005 per quality point.
    let gem_quality = env.output.get("GemQuality");
    if gem_quality > 0.0 {
        let q_bonus = 1.0 + gem_quality * 0.005;
        steps.push(
            BreakdownStep::label("Quality")
                .with_value(q_bonus)
                .with_explain(format!("+0.5% per quality, level Q{gem_quality:.0}")),
        );
    }

    steps.push(
        BreakdownStep::label("Average hit")
            .with_value(avg)
            .with_explain(format!("min {hit_min:.0} – max {hit_max:.0}")),
    );

    Breakdown {
        output_key: "MainSkillAverageHit".to_owned(),
        total: avg,
        steps,
    }
}

fn damage_average_hit_with_crit(env: &Env, key: &str) -> Breakdown {
    let avg = env.output.get("MainSkillAverageHit");
    let crit_chance = env.output.get("MainSkillCritChance") / 100.0;
    let crit_multi = env.output.get("CritMultiplier").max(1.0);
    let crit_factor = (1.0 - crit_chance) + crit_chance * crit_multi;
    let with_crit = avg * crit_factor;

    let mut steps = Vec::new();
    steps.push(
        BreakdownStep::label("Average hit (no crit)")
            .with_value(avg)
            .with_explain("see MainSkillAverageHit".to_owned()),
    );
    steps.push(
        BreakdownStep::label("Crit chance")
            .with_value(crit_chance)
            .with_explain(format!("{:.2}%", crit_chance * 100.0)),
    );
    steps.push(
        BreakdownStep::label("Crit multiplier")
            .with_value(crit_multi)
            .with_explain(format!("{:.0}%", crit_multi * 100.0)),
    );
    steps.push(
        BreakdownStep::label("Crit factor")
            .with_value(crit_factor)
            .with_explain(format!(
                "(1 - {chance:.4}) + {chance:.4} × {multi:.3}",
                chance = crit_chance,
                multi = crit_multi,
            )),
    );
    let total_value = env.output.get(key);
    let displayed = if total_value.abs() > 1e-9 {
        total_value
    } else {
        with_crit
    };
    steps.push(
        BreakdownStep::label("Average with crit")
            .with_value(displayed)
            .with_explain(format!("avg × crit_factor = {avg:.0} × {crit_factor:.3}")),
    );
    Breakdown {
        output_key: key.to_owned(),
        total: displayed,
        steps,
    }
}

fn damage_dps(env: &Env, key: &str) -> Breakdown {
    let avg = env.output.get("MainSkillAverageHit");
    let crit_factor = env.output.get("CritEffect").max(1.0);
    let avg_with_crit = avg * crit_factor;
    let res_factor_raw = if avg_with_crit > 1e-9 {
        env.output.get("MainSkillAverageHitAfterResist") / avg_with_crit
    } else {
        1.0
    };
    let after_resist = env.output.get("MainSkillAverageHitAfterResist");
    let after_shock = env.output.get("MainSkillAverageHitAfterShock");
    let after_accuracy = env.output.get("MainSkillAverageHitAfterAccuracy");
    let speed = env.output.get("MainSkillSpeed");
    let dps = env.output.get(key);

    let mut steps = Vec::new();
    steps.push(
        BreakdownStep::label("Average hit")
            .with_value(avg)
            .with_explain("pre-crit, pre-mitigation".to_owned()),
    );
    steps.push(
        BreakdownStep::label("Crit factor")
            .with_value(crit_factor)
            .with_explain(format!(
                "× crit chance ({:.2}%) and multiplier ({:.0}%)",
                env.output.get("MainSkillCritChance"),
                env.output.get("CritMultiplier") * 100.0,
            )),
    );
    if (res_factor_raw - 1.0).abs() > 1e-9 {
        steps.push(
            BreakdownStep::label("Enemy resist / armour")
                .with_value(res_factor_raw)
                .with_explain(format!(
                    "effective resist {:.0}% → after-mitigation {:.0}",
                    env.output.get("MainSkillEnemyEffectiveResist"),
                    after_resist,
                )),
        );
    }
    if after_shock > 0.0 && (after_shock - after_resist).abs() > 1e-9 {
        let shock_mult = after_shock / after_resist.max(1e-9);
        steps.push(
            BreakdownStep::label("Shock multiplier")
                .with_value(shock_mult)
                .with_explain(format!("{:.3}", shock_mult)),
        );
    }
    if after_accuracy > 0.0 && (after_accuracy - after_shock).abs() > 1e-9 {
        let hit_chance = after_accuracy / after_shock.max(1e-9);
        steps.push(
            BreakdownStep::label("Hit chance")
                .with_value(hit_chance)
                .with_explain(format!("{:.1}%", hit_chance * 100.0)),
        );
    }
    steps.push(
        BreakdownStep::label("Cast / attack rate")
            .with_value(speed)
            .with_explain(format!("{speed:.3} per second")),
    );
    steps.push(
        BreakdownStep::label("DPS")
            .with_value(dps)
            .with_explain("× speed".to_owned()),
    );
    Breakdown {
        output_key: key.to_owned(),
        total: dps,
        steps,
    }
}

fn damage_full_dps(env: &Env) -> Breakdown {
    let main = env.output.get("MainSkillDPS");
    let bleed = env.output.get("BleedDPS");
    let poison = env.output.get("PoisonDPS");
    let ignite = env.output.get("IgniteDPS");
    let impale = env.output.get("ImpaleDPS");
    let total = env.output.get("FullDPS");

    let mut steps = Vec::new();
    steps.push(BreakdownStep::label("Hit DPS").with_value(main));
    if bleed > 0.0 {
        steps.push(BreakdownStep::label("+ Bleed DPS").with_value(bleed));
    }
    if poison > 0.0 {
        steps.push(BreakdownStep::label("+ Poison DPS").with_value(poison));
    }
    if ignite > 0.0 {
        steps.push(BreakdownStep::label("+ Ignite DPS").with_value(ignite));
    }
    if impale > 0.0 {
        steps.push(BreakdownStep::label("+ Impale DPS").with_value(impale));
    }
    steps.push(
        BreakdownStep::label("FullDPS")
            .with_value(total)
            .with_explain("hit + ailment + impale".to_owned()),
    );
    Breakdown {
        output_key: "FullDPS".to_owned(),
        total,
        steps,
    }
}

// --- Speed ----------------------------------------------------------

fn speed_simple_mult(env: &Env, mod_name: &str, output_key: &str, label: &str) -> Breakdown {
    let cfg = QueryCfg::default();
    let inc_total = env.mod_db.sum(ModType::Inc, &cfg, &env.state, mod_name);
    let more_total = env.mod_db.more(&cfg, &env.state, mod_name);
    let total = env.output.get(output_key);

    let mut steps = Vec::new();
    steps.push(
        BreakdownStep::label("Base")
            .with_value(1.0)
            .with_explain("100% baseline".to_owned()),
    );

    let mut inc_mods: Vec<ModSource> = env
        .mod_db
        .iter_named(mod_name)
        .filter(|m| m.kind == ModType::Inc)
        .map(ModSource::from_mod)
        .collect();
    if !inc_mods.is_empty() || inc_total != 0.0 {
        // Sort by absolute contribution so the biggest boost is at the
        // top — matches PoB's display ordering.
        inc_mods.sort_by(|a, b| {
            b.value
                .unwrap_or(0.0)
                .abs()
                .partial_cmp(&a.value.unwrap_or(0.0).abs())
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        steps.push(
            BreakdownStep::label("Increased")
                .with_value(1.0 + inc_total / 100.0)
                .with_explain(format!("{inc_total:+.0}% sum"))
                .with_sources(inc_mods),
        );
    }

    let mut more_mods: Vec<ModSource> = env
        .mod_db
        .iter_named(mod_name)
        .filter(|m| m.kind == ModType::More)
        .map(ModSource::from_mod)
        .collect();
    if !more_mods.is_empty() || (more_total - 1.0).abs() > 1e-9 {
        more_mods.sort_by(|a, b| {
            b.value
                .unwrap_or(0.0)
                .abs()
                .partial_cmp(&a.value.unwrap_or(0.0).abs())
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        steps.push(
            BreakdownStep::label("More")
                .with_value(more_total)
                .with_explain("multiplicative".to_owned())
                .with_sources(more_mods),
        );
    }

    steps.push(
        BreakdownStep::label(label)
            .with_value(total)
            .with_explain(format!(
                "(1 + {inc_total:.0}%) × {more_total:.3} = {total:.3}"
            )),
    );

    Breakdown {
        output_key: output_key.to_owned(),
        total,
        steps,
    }
}

fn speed_main_skill(env: &Env, key: &str) -> Breakdown {
    let cps = env.output.get("MainSkillSpeed");
    let attack_mult = env.output.get("AttackSpeedMult");
    let cast_mult = env.output.get("CastSpeedMult");
    // Pick whichever multiplier was actually applied by detecting which
    // is non-default (or just present).
    let speed_mult = if (cast_mult - 1.0).abs() > (attack_mult - 1.0).abs() {
        cast_mult
    } else {
        attack_mult
    };
    let baseline = if speed_mult > 1e-9 {
        cps / speed_mult
    } else {
        1.0
    };

    let mut steps = Vec::new();
    steps.push(
        BreakdownStep::label("Baseline rate")
            .with_value(baseline)
            .with_explain("from skill cast time / weapon attack rate".to_owned()),
    );

    // INC + MORE come from the chosen pool (cast or attack).
    let pool = if speed_mult == cast_mult {
        "CastSpeed"
    } else {
        "AttackSpeed"
    };
    let cfg = QueryCfg::default();
    let inc_total = env.mod_db.sum(ModType::Inc, &cfg, &env.state, pool);
    let more_total = env.mod_db.more(&cfg, &env.state, pool);

    let mut inc_mods: Vec<ModSource> = env
        .mod_db
        .iter_named(pool)
        .filter(|m| m.kind == ModType::Inc)
        .map(ModSource::from_mod)
        .collect();
    inc_mods.sort_by(|a, b| {
        b.value
            .unwrap_or(0.0)
            .abs()
            .partial_cmp(&a.value.unwrap_or(0.0).abs())
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    if !inc_mods.is_empty() || inc_total != 0.0 {
        steps.push(
            BreakdownStep::label("Increased")
                .with_value(1.0 + inc_total / 100.0)
                .with_explain(format!("{pool}: {inc_total:+.0}%"))
                .with_sources(inc_mods),
        );
    }
    let mut more_mods: Vec<ModSource> = env
        .mod_db
        .iter_named(pool)
        .filter(|m| m.kind == ModType::More)
        .map(ModSource::from_mod)
        .collect();
    more_mods.sort_by(|a, b| {
        b.value
            .unwrap_or(0.0)
            .abs()
            .partial_cmp(&a.value.unwrap_or(0.0).abs())
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    if !more_mods.is_empty() || (more_total - 1.0).abs() > 1e-9 {
        steps.push(
            BreakdownStep::label("More")
                .with_value(more_total)
                .with_explain(format!("{pool}: ×{more_total:.3}"))
                .with_sources(more_mods),
        );
    }
    steps.push(
        BreakdownStep::label(if pool == "CastSpeed" {
            "Casts per second"
        } else {
            "Attacks per second"
        })
        .with_value(cps)
        .with_explain(format!("{baseline:.3} × {speed_mult:.3} = {cps:.3}")),
    );
    Breakdown {
        output_key: key.to_owned(),
        total: cps,
        steps,
    }
}

// --- Crit -----------------------------------------------------------

fn crit_chance(env: &Env, key: &str) -> Breakdown {
    let cfg = QueryCfg::default();
    let crit_inc = env.mod_db.sum(ModType::Inc, &cfg, &env.state, "CritChance");
    let crit_base = env
        .mod_db
        .sum(ModType::Base, &cfg, &env.state, "CritChance");
    // We can't know the skill's intrinsic crit from env alone — the
    // basic-stats pass uses 5.0 as the floor; perform_skill_dps swaps
    // in the skill's own value. Use MainSkillCritChance ÷ (1+inc) as
    // the recovered base when MainSkillCritChance is set; fall back to
    // 5.0 + base.
    let main_skill_crit = env.output.get("MainSkillCritChance");
    let total = env.output.get(key);
    let intrinsic = if main_skill_crit > 0.0 && (1.0 + crit_inc / 100.0) > 1e-9 {
        // For MainSkillCritChance we have to back out hit_chance for
        // attacks. Skip that nuance — the visible "intrinsic" line is a
        // diagnostic, not a load-bearing number.
        main_skill_crit / (1.0 + crit_inc / 100.0) - crit_base
    } else if total > 0.0 {
        total / (1.0 + crit_inc / 100.0) - crit_base
    } else {
        5.0
    };

    let mut steps = Vec::new();
    steps.push(
        BreakdownStep::label("Skill base crit")
            .with_value(intrinsic.max(0.0))
            .with_explain("intrinsic chance for the active skill (5% default)".to_owned()),
    );

    if crit_base != 0.0 {
        let base_mods: Vec<ModSource> = env
            .mod_db
            .iter_named("CritChance")
            .filter(|m| m.kind == ModType::Base)
            .map(ModSource::from_mod)
            .collect();
        steps.push(
            BreakdownStep::label("+ flat crit")
                .with_value(crit_base)
                .with_explain(format!("+{crit_base:.2}% additional base"))
                .with_sources(base_mods),
        );
    }

    if crit_inc != 0.0 {
        let inc_mods: Vec<ModSource> = env
            .mod_db
            .iter_named("CritChance")
            .filter(|m| m.kind == ModType::Inc)
            .map(ModSource::from_mod)
            .collect();
        steps.push(
            BreakdownStep::label("Increased crit")
                .with_value(1.0 + crit_inc / 100.0)
                .with_explain(format!("{crit_inc:+.0}% sum"))
                .with_sources(inc_mods),
        );
    }

    let chance_after = ((intrinsic.max(0.0) + crit_base) * (1.0 + crit_inc / 100.0)).max(0.0);
    steps.push(
        BreakdownStep::label("Pre-hit-chance crit")
            .with_value(chance_after)
            .with_explain(format!(
                "= ({intrinsic:.2} + {crit_base:.2}) × {factor:.3}",
                factor = 1.0 + crit_inc / 100.0,
            )),
    );

    if key == "CritChance" {
        // Character-level CritChance also folds in HitChance for attacks.
        let hit_chance_pct = env.output.get("MainSkillHitChance");
        if hit_chance_pct > 0.0 && (hit_chance_pct - 100.0).abs() > 1e-9 {
            steps.push(
                BreakdownStep::label("× hit chance")
                    .with_value(hit_chance_pct / 100.0)
                    .with_explain(format!(
                        "attack crit only triggers on a successful hit ({hit_chance_pct:.0}%)"
                    )),
            );
        }
    }

    steps.push(
        BreakdownStep::label("Effective crit chance")
            .with_value(total)
            .with_explain(format!("{total:.2}%")),
    );
    Breakdown {
        output_key: key.to_owned(),
        total,
        steps,
    }
}

fn crit_multiplier(env: &Env) -> Breakdown {
    let cfg = QueryCfg::default();
    let base_extra = env
        .mod_db
        .sum(ModType::Base, &cfg, &env.state, "CritMultiplier");
    let total = env.output.get("CritMultiplier");
    let total_pct = total * 100.0;

    let mut steps = Vec::new();
    steps.push(
        BreakdownStep::label("Base crit multiplier")
            .with_value(1.5)
            .with_explain("PoE baseline: 150% damage on crit".to_owned()),
    );
    if base_extra != 0.0 {
        let base_mods: Vec<ModSource> = env
            .mod_db
            .iter_named("CritMultiplier")
            .filter(|m| m.kind == ModType::Base)
            .map(ModSource::from_mod)
            .collect();
        steps.push(
            BreakdownStep::label("+ additional crit damage")
                .with_value(base_extra / 100.0)
                .with_explain(format!("+{base_extra:.0}% (BASE mods)"))
                .with_sources(base_mods),
        );
    }
    steps.push(
        BreakdownStep::label("Crit multiplier")
            .with_value(total)
            .with_explain(format!("= {total_pct:.0}%")),
    );
    Breakdown {
        output_key: "CritMultiplier".to_owned(),
        total,
        steps,
    }
}

fn crit_effect(env: &Env) -> Breakdown {
    let chance = env.output.get("MainSkillCritChance") / 100.0;
    let multi = env.output.get("CritMultiplier").max(1.0);
    let factor = (1.0 - chance) + chance * multi;
    let stored = env.output.get("CritEffect");
    let displayed = if stored > 0.0 { stored } else { factor };

    let mut steps = Vec::new();
    steps.push(
        BreakdownStep::label("Crit chance")
            .with_value(chance)
            .with_explain(format!("{:.2}%", chance * 100.0)),
    );
    steps.push(
        BreakdownStep::label("Crit multiplier")
            .with_value(multi)
            .with_explain(format!("{:.0}%", multi * 100.0)),
    );
    steps.push(
        BreakdownStep::label("Crit effect")
            .with_value(displayed)
            .with_explain(format!(
                "(1 − {chance:.4}) + {chance:.4} × {multi:.3} = {displayed:.4}"
            )),
    );
    Breakdown {
        output_key: "CritEffect".to_owned(),
        total: displayed,
        steps,
    }
}

// --- Resists --------------------------------------------------------

/// Issue #34 follow-up: re-derive an elemental resist's effective
/// value (Fire / Cold / Lightning). Mirrors `perform_basic_stats`:
///
///   raw   = Σ BASE(<elem>Resist) + Σ BASE(ElementalResist) + level penalty
///   cap   = 75 + Σ BASE(<elem>ResistMax)
///   total = min(raw, cap)
///
/// The level penalty mirrors PoE's Act 5 / Act 10 -30 / -60 story
/// hits (PoB applies -60 for any character of level ≥ 68). MK2's
/// engine bakes this into the BASE sum implicitly; the breakdown
/// surfaces it separately as a recovered value so users can see
/// where their starting hole comes from.
fn elemental_resist(env: &Env, elem: &str) -> Breakdown {
    let cfg = QueryCfg::default();
    let resist_key = format!("{elem}Resist");
    let max_key = format!("{elem}ResistMax");
    let total_key = format!("{elem}ResistTotal");

    let elem_base = env.mod_db.sum(ModType::Base, &cfg, &env.state, &resist_key);
    let umbrella = env
        .mod_db
        .sum(ModType::Base, &cfg, &env.state, "ElementalResist");
    // Recover the level penalty from the difference between the
    // computed `<elem>Resist` output and the BASE sums. The penalty
    // is -60 for level ≥ 68 characters and 0 otherwise; this lets
    // the breakdown stay accurate even if the formula evolves.
    let raw = env.output.get(&resist_key);
    let level_penalty = raw - elem_base - umbrella;
    let max = env.output.get(&max_key);
    let max_bonus = env.mod_db.sum(ModType::Base, &cfg, &env.state, &max_key);
    let total = env.output.get(&total_key);

    let mut steps = Vec::new();
    let elem_mods: Vec<ModSource> = env
        .mod_db
        .iter_named(&resist_key)
        .filter(|m| m.kind == ModType::Base)
        .map(ModSource::from_mod)
        .collect();
    steps.push(
        BreakdownStep::label(format!("{elem} Resist BASE"))
            .with_value(elem_base)
            .with_explain(format!("+{elem_base:.0}% from {elem}Resist BASE mods"))
            .with_sources(elem_mods),
    );

    if umbrella.abs() > 1e-9 {
        let umbrella_mods: Vec<ModSource> = env
            .mod_db
            .iter_named("ElementalResist")
            .filter(|m| m.kind == ModType::Base)
            .map(ModSource::from_mod)
            .collect();
        steps.push(
            BreakdownStep::label("Elemental Resist BASE")
                .with_value(umbrella)
                .with_explain(format!("+{umbrella:.0}% umbrella adders"))
                .with_sources(umbrella_mods),
        );
    }

    if level_penalty.abs() > 1e-9 {
        steps.push(
            BreakdownStep::label("Level penalty")
                .with_value(level_penalty)
                .with_explain(
                    "Act 5 / Act 10 story penalty (-30 / -60 by post-act level threshold)"
                        .to_owned(),
                ),
        );
    }

    steps.push(
        BreakdownStep::label("Cap")
            .with_value(max)
            .with_explain(format!("75 + {max_bonus:.0}% from {elem}ResistMax mods")),
    );

    steps.push(
        BreakdownStep::label(format!("{elem} Resist (effective)"))
            .with_value(total)
            .with_explain(format!("min({raw:.0}, {max:.0}) = {total:.0}")),
    );

    Breakdown {
        output_key: total_key,
        total,
        steps,
    }
}

/// Issue #34 follow-up: chaos resist mirrors the elemental shape but
/// with no umbrella `ElementalResist` adder (chaos doesn't pick up
/// `+all elemental resistances` mods). Same level penalty + cap
/// formulation.
fn chaos_resist(env: &Env) -> Breakdown {
    let cfg = QueryCfg::default();
    let chaos_base = env
        .mod_db
        .sum(ModType::Base, &cfg, &env.state, "ChaosResist");
    let raw = env.output.get("ChaosResist");
    let level_penalty = raw - chaos_base;
    let max = env.output.get("ChaosResistMax");
    let max_bonus = env
        .mod_db
        .sum(ModType::Base, &cfg, &env.state, "ChaosResistMax");
    let total = env.output.get("ChaosResistTotal");

    let mut steps = Vec::new();
    let chaos_mods: Vec<ModSource> = env
        .mod_db
        .iter_named("ChaosResist")
        .filter(|m| m.kind == ModType::Base)
        .map(ModSource::from_mod)
        .collect();
    steps.push(
        BreakdownStep::label("Chaos Resist BASE")
            .with_value(chaos_base)
            .with_explain(format!("+{chaos_base:.0}% from ChaosResist BASE mods"))
            .with_sources(chaos_mods),
    );

    if level_penalty.abs() > 1e-9 {
        steps.push(
            BreakdownStep::label("Level penalty")
                .with_value(level_penalty)
                .with_explain(
                    "Act 5 / Act 10 story penalty (-30 / -60 by post-act level threshold)"
                        .to_owned(),
                ),
        );
    }

    steps.push(
        BreakdownStep::label("Cap")
            .with_value(max)
            .with_explain(format!("75 + {max_bonus:.0}% from ChaosResistMax mods")),
    );

    steps.push(
        BreakdownStep::label("Chaos Resist (effective)")
            .with_value(total)
            .with_explain(format!("min({raw:.0}, {max:.0}) = {total:.0}")),
    );

    Breakdown {
        output_key: "ChaosResistTotal".to_owned(),
        total,
        steps,
    }
}

/// Issue #34 follow-up: re-derive a damage-avoidance chance that
/// follows the `min(Σ BASE, cap)` shape — Block / Spell Block / Spell
/// Suppression. `cap_key` names a `Max`-style mod that raises the
/// default cap by `Σ BASE(cap_key)`; pass `None` for stats with a
/// fixed cap (e.g. Spell Suppression at 100).
///
/// The output step-list mirrors `chaos_resist`: the BASE step
/// surfaces every contributing mod, an optional cap step shows the
/// derivation when `cap_key` is present, and the effective step
/// reports the `min(raw, cap)` clamp.
fn capped_chance(
    env: &Env,
    key: &str,
    cap_key: Option<&str>,
    label: &str,
    default_cap: f64,
) -> Breakdown {
    let cfg = QueryCfg::default();
    let raw = env.mod_db.sum(ModType::Base, &cfg, &env.state, key);
    let cap = match cap_key {
        Some(ck) => default_cap + env.mod_db.sum(ModType::Base, &cfg, &env.state, ck),
        None => default_cap,
    };
    let total = env.output.get(key);

    let mut steps = Vec::new();
    let base_mods: Vec<ModSource> = env
        .mod_db
        .iter_named(key)
        .filter(|m| m.kind == ModType::Base)
        .map(ModSource::from_mod)
        .collect();
    steps.push(
        BreakdownStep::label(format!("{label} BASE"))
            .with_value(raw)
            .with_explain(format!("+{raw:.0}% from {key} BASE mods"))
            .with_sources(base_mods),
    );

    if let Some(ck) = cap_key {
        let cap_bonus = env.mod_db.sum(ModType::Base, &cfg, &env.state, ck);
        let cap_mods: Vec<ModSource> = env
            .mod_db
            .iter_named(ck)
            .filter(|m| m.kind == ModType::Base)
            .map(ModSource::from_mod)
            .collect();
        steps.push(
            BreakdownStep::label("Cap")
                .with_value(cap)
                .with_explain(format!("{default_cap:.0} + {cap_bonus:.0}% from {ck} mods"))
                .with_sources(cap_mods),
        );
    } else {
        steps.push(
            BreakdownStep::label("Cap")
                .with_value(cap)
                .with_explain(format!("hard cap at {default_cap:.0}%")),
        );
    }

    steps.push(
        BreakdownStep::label(format!("{label} (effective)"))
            .with_value(total)
            .with_explain(format!("min({raw:.0}, {cap:.0}) = {total:.0}")),
    );

    Breakdown {
        output_key: key.to_owned(),
        total,
        steps,
    }
}

// --- Helpers --------------------------------------------------------

/// Issue #34 follow-up: re-derive a pool stat that has no primary
/// attribute driver — Energy Shield (drawn from item-base intrinsics +
/// the Int → Inc EnergyShield wiring already captured as INC mods),
/// Ward, etc. Walks Base → Increased → More → Final, with each step's
/// contributor mods enumerated under its source list. Pure helper —
/// shape mirrors `pool_with_attribute` but skips the attribute step.
fn pool_basic(env: &Env, key: &str) -> Breakdown {
    let cfg = QueryCfg::default();
    let pool_base = env.mod_db.sum(ModType::Base, &cfg, &env.state, key);
    let inc_total = env.mod_db.sum(ModType::Inc, &cfg, &env.state, key);
    let more_total = env.mod_db.more(&cfg, &env.state, key);
    let total = env.output.get(key);

    let mut steps = Vec::new();
    let base_mods: Vec<ModSource> = env
        .mod_db
        .iter_named(key)
        .filter(|m| m.kind == ModType::Base)
        .map(ModSource::from_mod)
        .collect();
    steps.push(
        BreakdownStep::label("Base")
            .with_value(pool_base)
            .with_explain(format!("+{pool_base:.0} from BASE mods"))
            .with_sources(base_mods),
    );

    let mut inc_mods: Vec<ModSource> = env
        .mod_db
        .iter_named(key)
        .filter(|m| m.kind == ModType::Inc)
        .map(ModSource::from_mod)
        .collect();
    if !inc_mods.is_empty() || inc_total != 0.0 {
        inc_mods.sort_by(|a, b| {
            b.value
                .unwrap_or(0.0)
                .abs()
                .partial_cmp(&a.value.unwrap_or(0.0).abs())
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        steps.push(
            BreakdownStep::label("Increased")
                .with_value(1.0 + inc_total / 100.0)
                .with_explain(format!("{inc_total:+.0}% sum"))
                .with_sources(inc_mods),
        );
    }

    let mut more_mods: Vec<ModSource> = env
        .mod_db
        .iter_named(key)
        .filter(|m| m.kind == ModType::More)
        .map(ModSource::from_mod)
        .collect();
    if !more_mods.is_empty() || (more_total - 1.0).abs() > 1e-9 {
        more_mods.sort_by(|a, b| {
            b.value
                .unwrap_or(0.0)
                .abs()
                .partial_cmp(&a.value.unwrap_or(0.0).abs())
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        steps.push(
            BreakdownStep::label("More")
                .with_value(more_total)
                .with_explain("multiplicative".to_owned())
                .with_sources(more_mods),
        );
    }

    steps.push(
        BreakdownStep::label(key)
            .with_value(total)
            .with_explain(format!(
                "{pool_base:.0} × (1 + {inc_total:.0}%) × {more_total:.3} = {total:.0}"
            )),
    );

    Breakdown {
        output_key: key.to_owned(),
        total,
        steps,
    }
}

/// Issue #34 follow-up: re-derive `ImpaleDPS`. PoB:
/// `stored × (effect/100) × stacks × (chance/100) × cps`
/// (perform.rs:4248). Phys-attack builds running the impale support
/// want to see all four knobs that drive the damage. Stack count
/// (default 5; raised by `ImpaleStacksMax`) goes in the explain
/// text rather than its own step since most builds don't tune it.
fn impale_dps(env: &Env) -> Breakdown {
    let stored = env.output.get("ImpaleStoredHitAvg");
    let effect_pct = env.output.get("ImpaleEffect");
    let chance_pct = env.output.get("ImpaleChance");
    let cps = env.output.get("MainSkillSpeed");
    let total = env.output.get("ImpaleDPS");
    let effect_factor = effect_pct / 100.0;
    let chance_factor = (chance_pct / 100.0).clamp(0.0, 1.0);

    let mut steps = Vec::new();
    steps.push(
        BreakdownStep::label("Stored hit average")
            .with_value(stored)
            .with_explain(format!("{stored:.0} (post-crit, pre-mitigation phys hit)")),
    );
    steps.push(
        BreakdownStep::label("Effect per stack")
            .with_value(effect_factor)
            .with_explain(format!(
                "{effect_pct:.0}% per stack × default 5 stacks (raise via ImpaleStacksMax)"
            )),
    );
    steps.push(
        BreakdownStep::label("Impale chance")
            .with_value(chance_factor)
            .with_explain(format!("{chance_pct:.0}% / 100 = {chance_factor:.3}")),
    );
    steps.push(
        BreakdownStep::label("Casts per second")
            .with_value(cps)
            .with_explain(format!("{cps:.2} from MainSkillSpeed")),
    );
    steps.push(
        BreakdownStep::label("ImpaleDPS")
            .with_value(total)
            .with_explain(format!(
                "{stored:.0} × {effect_factor:.3} × 5 stacks × {chance_factor:.3} × {cps:.2} = {total:.0}"
            )),
    );

    Breakdown {
        output_key: "ImpaleDPS".to_owned(),
        total,
        steps,
    }
}

/// Issue #34 follow-up: re-derive single-application ailment DPS
/// (Bleed / Ignite). PoB: `per_application × chance` — both
/// ailments are single-stack (longest overrides), so the long-run
/// DPS is `chance × per_application`. Per-application back-derived
/// from the stored DPS / chance since the perform pass doesn't
/// expose it directly.
///
/// Shared helper because the formula is identical apart from which
/// chance key is read and the rendered label.
fn single_app_ailment_dps(env: &Env, dps_key: &str, chance_key: &str, label: &str) -> Breakdown {
    let total = env.output.get(dps_key);
    let chance_pct = env.output.get(chance_key);
    let chance_factor = (chance_pct / 100.0).clamp(0.0, 1.0);
    let per_application = if chance_factor > 1e-9 {
        total / chance_factor
    } else {
        0.0
    };

    let mut steps = Vec::new();
    steps.push(
        BreakdownStep::label("Per-application damage")
            .with_value(per_application)
            .with_explain(format!(
                "{per_application:.0} (= {dps_key} / chance back-derived)"
            )),
    );
    steps.push(
        BreakdownStep::label(format!("{label} chance"))
            .with_value(chance_factor)
            .with_explain(format!("{chance_pct:.0}% / 100 = {chance_factor:.3}")),
    );
    steps.push(
        BreakdownStep::label(dps_key)
            .with_value(total)
            .with_explain(format!(
                "{per_application:.0} × {chance_factor:.3} = {total:.0}"
            )),
    );

    Breakdown {
        output_key: dps_key.to_owned(),
        total,
        steps,
    }
}

/// Issue #34 follow-up: re-derive `PoisonDPS`. PoB:
/// `per_stack × steady_state_stacks` (perform.rs:3630). The
/// per-stack damage isn't stored separately on output, so the
/// helper back-derives it from `PoisonDPS / PoisonStacks`. Both
/// contributors are surfaced so the user can see whether their
/// poison reading comes from per-hit damage or stack-count
/// investment (cast speed, duration, chance).
fn poison_dps(env: &Env) -> Breakdown {
    let total = env.output.get("PoisonDPS");
    let stacks = env.output.get("PoisonStacks");
    let per_stack = if stacks > 1e-9 { total / stacks } else { 0.0 };

    let mut steps = Vec::new();
    steps.push(
        BreakdownStep::label("Per-stack damage")
            .with_value(per_stack)
            .with_explain(format!(
                "{per_stack:.0} (= PoisonDPS / PoisonStacks back-derived)"
            )),
    );
    steps.push(
        BreakdownStep::label("Steady-state stacks")
            .with_value(stacks)
            .with_explain(format!(
                "{stacks:.1} from cast_rate × duration × chance, capped at PoisonStackLimit"
            )),
    );
    steps.push(
        BreakdownStep::label("PoisonDPS")
            .with_value(total)
            .with_explain(format!("{per_stack:.0} × {stacks:.1} = {total:.0}")),
    );

    Breakdown {
        output_key: "PoisonDPS".to_owned(),
        total,
        steps,
    }
}

/// Issue #34 follow-up: re-derive `TotalEHP`. PoB:
/// `NumberOfDamagingHits × totalEnemyDamageIn` (perform.rs:1440).
/// PoB's headline defensive number — folds the Pinnacle Boss's
/// mixed-element damage profile into a single value. Distinct from
/// per-element EHP; usable as the build's "how tanky am I" reading.
fn total_ehp(env: &Env) -> Breakdown {
    let hits = env.output.get("NumberOfDamagingHits");
    let damage_in = env.output.get("totalEnemyDamageIn");
    let total = env.output.get("TotalEHP");

    let mut steps = Vec::new();
    steps.push(
        BreakdownStep::label("Hits to die")
            .with_value(hits)
            .with_explain(format!("{hits:.2} (see NumberOfDamagingHits)")),
    );
    steps.push(
        BreakdownStep::label("Boss damage per hit")
            .with_value(damage_in)
            .with_explain(format!(
                "{damage_in:.0} from PoB's standard Pinnacle Boss preset (4 elements + chaos)"
            )),
    );
    steps.push(
        BreakdownStep::label("TotalEHP")
            .with_value(total)
            .with_explain(format!("{hits:.2} × {damage_in:.0} = {total:.0}")),
    );

    Breakdown {
        output_key: "TotalEHP".to_owned(),
        total,
        steps,
    }
}

/// Issue #34 follow-up: re-derive `EHPSurvivalTime`. PoB:
/// `hits_to_die × enemySkillTime` (perform.rs:1442). Two
/// contributors — hits-to-die and the boss tick rate. Lets the
/// user reason about whether to invest in raw EHP or in mobility /
/// dodge affixes.
fn ehp_survival_time(env: &Env) -> Breakdown {
    let hits = env.output.get("NumberOfDamagingHits");
    let tick = env.output.get("enemySkillTime");
    let total = env.output.get("EHPSurvivalTime");

    let mut steps = Vec::new();
    steps.push(
        BreakdownStep::label("Hits to die")
            .with_value(hits)
            .with_explain(format!("{hits:.2} (see NumberOfDamagingHits)")),
    );
    steps.push(
        BreakdownStep::label("Boss tick")
            .with_value(tick)
            .with_explain(format!(
                "{tick:.2}s — PoB's standard Pinnacle Boss tick rate"
            )),
    );
    steps.push(
        BreakdownStep::label("EHPSurvivalTime")
            .with_value(total)
            .with_explain(format!("{hits:.2} × {tick:.2} = {total:.2}s")),
    );

    Breakdown {
        output_key: "EHPSurvivalTime".to_owned(),
        total,
        steps,
    }
}

/// Issue #34 follow-up: re-derive `NumberOfDamagingHits`. PoB:
/// `pool / total_taken_hit` (perform.rs:1436). Defensive builds
/// tuning EHP investment want to see whether their hits-to-die
/// figure is constrained by pool or by mitigation. Two-component
/// breakdown surfacing both.
fn number_of_damaging_hits(env: &Env) -> Breakdown {
    let life = env.output.get("Life");
    let es = env.output.get("EnergyShield");
    let ward = env.output.get("Ward");
    let pool = (life + es + ward).max(1.0);
    let taken = env.output.get("totalTakenHit");
    let total = env.output.get("NumberOfDamagingHits");

    let mut steps = Vec::new();
    steps.push(
        BreakdownStep::label("Pool")
            .with_value(pool)
            .with_explain(format!(
                "Life {life:.0} + EnergyShield {es:.0} + Ward {ward:.0} = {pool:.0}"
            )),
    );
    steps.push(
        BreakdownStep::label("Total taken per hit")
            .with_value(taken)
            .with_explain(format!(
                "{taken:.0} from PoB's standard Pinnacle Boss mixed-element hit"
            )),
    );
    steps.push(
        BreakdownStep::label("NumberOfDamagingHits")
            .with_value(total)
            .with_explain(format!("{pool:.0} / {taken:.0} = {total:.2}")),
    );

    Breakdown {
        output_key: "NumberOfDamagingHits".to_owned(),
        total,
        steps,
    }
}

/// Issue #34 follow-up: re-derive `AverageEHP` (arithmetic mean of
/// the five per-element EHPs). Surfaces all five contributors so the
/// user can see which damage type is dragging the average.
fn average_ehp(env: &Env) -> Breakdown {
    let phys = env.output.get("PhysicalEHP");
    let fire = env.output.get("FireEHP");
    let cold = env.output.get("ColdEHP");
    let lightning = env.output.get("LightningEHP");
    let chaos = env.output.get("ChaosEHP");
    let total = env.output.get("AverageEHP");

    let mut steps = Vec::new();
    for (label, value) in [
        ("Physical", phys),
        ("Fire", fire),
        ("Cold", cold),
        ("Lightning", lightning),
        ("Chaos", chaos),
    ] {
        steps.push(
            BreakdownStep::label(label)
                .with_value(value)
                .with_explain(format!("{value:.0} EHP")),
        );
    }
    steps.push(
        BreakdownStep::label("AverageEHP")
            .with_value(total)
            .with_explain(format!(
                "({phys:.0} + {fire:.0} + {cold:.0} + {lightning:.0} + {chaos:.0}) / 5 = {total:.0}"
            )),
    );

    Breakdown {
        output_key: "AverageEHP".to_owned(),
        total,
        steps,
    }
}

/// Issue #34 follow-up: re-derive `MinimumEHP` (worst-case damage
/// type — the smallest of the five per-element EHPs). Surfaces all
/// five contributors and calls out the worst-case element by name in
/// the final step's explain text so the user knows which damage type
/// to invest defenses against.
fn minimum_ehp(env: &Env) -> Breakdown {
    let phys = env.output.get("PhysicalEHP");
    let fire = env.output.get("FireEHP");
    let cold = env.output.get("ColdEHP");
    let lightning = env.output.get("LightningEHP");
    let chaos = env.output.get("ChaosEHP");
    let total = env.output.get("MinimumEHP");

    let entries = [
        ("Physical", phys),
        ("Fire", fire),
        ("Cold", cold),
        ("Lightning", lightning),
        ("Chaos", chaos),
    ];
    let worst_label = entries
        .iter()
        .min_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal))
        .map(|(l, _)| *l)
        .unwrap_or("Physical");

    let mut steps = Vec::new();
    for (label, value) in entries {
        steps.push(
            BreakdownStep::label(label)
                .with_value(value)
                .with_explain(format!("{value:.0} EHP")),
        );
    }
    steps.push(
        BreakdownStep::label("MinimumEHP")
            .with_value(total)
            .with_explain(format!(
                "min of the five = {total:.0} ({worst_label} is the weakest)"
            )),
    );

    Breakdown {
        output_key: "MinimumEHP".to_owned(),
        total,
        steps,
    }
}

/// Issue #34 follow-up: shared helper for the per-element + chaos
/// EHP breakdowns. PoB's `perform_ehp` (perform.rs:4646) computes:
///
///   pool = Life + EnergyShield + Ward
///   resist_factor = 1 - (resist_total/100 - pen)
///   spell_mult    = 1 - 0.5 × suppress
///   spell_block_mult = 1 - spell_block
///   ele_taken = resist_factor × spell_mult × spell_block_mult
///   <Element>EHP = pool / max(ele_taken, 0.05)
///
/// `apply_pen = true` for Fire/Cold/Lightning (PoB's standard
/// Pinnacle Boss preset penetrates 3% of elemental resists);
/// `apply_pen = false` for chaos (no pen).
fn elemental_ehp(
    env: &Env,
    elem_label: &str,
    resist_key: &str,
    ehp_key: &str,
    apply_pen: bool,
) -> Breakdown {
    const PEN: f64 = 0.03;
    let life = env.output.get("Life");
    let es = env.output.get("EnergyShield");
    let ward = env.output.get("Ward");
    let pool = (life + es + ward).max(1.0);
    let resist_pct = env.output.get(resist_key);
    let resist_after_pen = if apply_pen {
        (resist_pct / 100.0).clamp(-2.0, 0.95) - PEN
    } else {
        (resist_pct / 100.0).clamp(-2.0, 0.95)
    }
    .clamp(-2.0, 0.95);
    let resist_factor = (1.0 - resist_after_pen).max(0.0);
    let suppress_pct = env.output.get("SpellSuppressionChance");
    let spell_block_pct = env.output.get("SpellBlockChance");
    let suppress_factor = (1.0 - 0.5 * suppress_pct / 100.0).max(0.0);
    let spell_block_factor = (1.0 - spell_block_pct / 100.0).max(0.0);
    let raw_taken = resist_factor * suppress_factor * spell_block_factor;
    let taken = raw_taken.max(0.05);
    let total = env.output.get(ehp_key);
    let floored = raw_taken < 0.05;

    let mut steps = Vec::new();
    steps.push(
        BreakdownStep::label("Pool")
            .with_value(pool)
            .with_explain(format!(
                "Life {life:.0} + EnergyShield {es:.0} + Ward {ward:.0} = {pool:.0}"
            )),
    );
    steps.push(
        BreakdownStep::label(format!("{elem_label} resist"))
            .with_value(resist_factor)
            .with_explain(if apply_pen {
                format!("1 - ({resist_pct:.0}% / 100 - {PEN:.2} pen) = {resist_factor:.3}")
            } else {
                format!("1 - {resist_pct:.0}% / 100 = {resist_factor:.3}")
            }),
    );
    steps.push(
        BreakdownStep::label("Spell suppression")
            .with_value(suppress_factor)
            .with_explain(format!(
                "1 - 0.5 × {suppress_pct:.0}% / 100 = {suppress_factor:.3}"
            )),
    );
    steps.push(
        BreakdownStep::label("Spell block")
            .with_value(spell_block_factor)
            .with_explain(format!(
                "1 - {spell_block_pct:.0}% / 100 = {spell_block_factor:.3}"
            )),
    );
    steps.push(
        BreakdownStep::label(ehp_key)
            .with_value(total)
            .with_explain(if floored {
                format!("{pool:.0} / max({raw_taken:.4}, 0.05) = {total:.0} (5% floor active)")
            } else {
                format!("{pool:.0} / {taken:.4} = {total:.0}")
            }),
    );

    Breakdown {
        output_key: ehp_key.to_owned(),
        total,
        steps,
    }
}

/// Issue #34 follow-up: re-derive `PhysicalEHP`. PoB:
///
///   pool = Life + EnergyShield + Ward
///   phys_taken = (1 - PhysicalDamageReduction/100) × (1 - BlockChance/100)
///   PhysicalEHP = pool / max(phys_taken, 0.05)
///
/// from `perform.rs:4646-4692`. Surfaces the pool, the two
/// multiplicative defensive factors, and the final reading. The
/// 5% floor on `phys_taken` (preventing infinite EHP from
/// extreme stacking) is documented in the explain text on the
/// final step when it kicks in.
fn physical_ehp(env: &Env) -> Breakdown {
    let life = env.output.get("Life");
    let es = env.output.get("EnergyShield");
    let ward = env.output.get("Ward");
    let pool = (life + es + ward).max(1.0);
    let phys_red_pct = env.output.get("PhysicalDamageReduction");
    let block_pct = env.output.get("BlockChance");
    let phys_red_factor = (1.0 - phys_red_pct / 100.0).max(0.0);
    let block_factor = (1.0 - block_pct / 100.0).max(0.0);
    let raw_taken = phys_red_factor * block_factor;
    let taken = raw_taken.max(0.05);
    let total = env.output.get("PhysicalEHP");
    let floored = (raw_taken - 0.05).abs() < 1e-9 || raw_taken < 0.05;

    let mut steps = Vec::new();
    steps.push(
        BreakdownStep::label("Pool")
            .with_value(pool)
            .with_explain(format!(
                "Life {life:.0} + EnergyShield {es:.0} + Ward {ward:.0} = {pool:.0}"
            )),
    );
    steps.push(
        BreakdownStep::label("Physical reduction")
            .with_value(phys_red_factor)
            .with_explain(format!(
                "1 - {phys_red_pct:.0}% / 100 = {phys_red_factor:.4}"
            )),
    );
    steps.push(
        BreakdownStep::label("Block")
            .with_value(block_factor)
            .with_explain(format!("1 - {block_pct:.0}% / 100 = {block_factor:.3}")),
    );
    steps.push(
        BreakdownStep::label("PhysicalEHP")
            .with_value(total)
            .with_explain(if floored {
                format!("{pool:.0} / max({raw_taken:.4}, 0.05) = {total:.0} (5% floor active)")
            } else {
                format!("{pool:.0} / {taken:.4} = {total:.0}")
            }),
    );

    Breakdown {
        output_key: "PhysicalEHP".to_owned(),
        total,
        steps,
    }
}

/// Issue #34 follow-up: re-derive `PhysicalDamageReduction`. PoB:
///
///   raw = armour / (armour + 12 × baseline_phys)
///   reduction% = min(raw × 100, 90)
///
/// from `perform.rs:1714-1717`. The baseline is fixed at 1000 in the
/// engine — surfacing it as a discrete step makes the diminishing-
/// returns curve obvious. The 90% cap is explicit so a min-maxer
/// stacking +1M armour can see why their reduction stops moving.
fn physical_damage_reduction(env: &Env) -> Breakdown {
    const BASELINE_PHYS: f64 = 1000.0;
    const REDUCTION_CAP: f64 = 90.0;
    let armour = env.output.get("Armour");
    let total = env.output.get("PhysicalDamageReduction");
    let capped = total >= REDUCTION_CAP - 1e-9;

    let mut steps = Vec::new();
    steps.push(
        BreakdownStep::label("Armour")
            .with_value(armour)
            .with_explain(format!("{armour:.0} from gear / passives / mods")),
    );
    steps.push(
        BreakdownStep::label("Baseline phys hit")
            .with_value(BASELINE_PHYS)
            .with_explain("PoB's standard 1000-pt baseline reference hit".to_owned()),
    );
    steps.push(
        BreakdownStep::label("PhysicalDamageReduction")
            .with_value(total)
            .with_explain(if capped {
                format!("{total:.0}% (hard cap — further armour wasted)")
            } else {
                format!(
                    "armour / (armour + 12 × baseline) × 100 = {armour:.0} / ({armour:.0} + 12000) × 100 = {total:.2}%"
                )
            }),
    );

    Breakdown {
        output_key: "PhysicalDamageReduction".to_owned(),
        total,
        steps,
    }
}

/// Issue #34 follow-up: re-derive `With<Ailment>DPS`. PoB stores the
/// rollup as `MainSkillDPS + <ailment>DPS` (see `perform.rs:4298-4301`).
/// One helper handles all four ailments; the caller passes the
/// rollup output key, the source ailment key, and the rendered
/// label.
///
/// Each breakdown surfaces:
/// - Hit DPS — `MainSkillDPS`, the hit damage layer
/// - <Ailment> DPS — the ailment damage layer (often zero on
///   non-bleed / non-poison / non-ignite builds, but kept in the
///   chain so the user can see why their With… reading collapses
///   to the hit value)
/// - Final — the stored output rollup
fn with_ailment_dps(
    env: &Env,
    rollup_key: &str,
    ailment_key: &str,
    ailment_label: &str,
) -> Breakdown {
    let hit = env.output.get("MainSkillDPS");
    let ailment = env.output.get(ailment_key);
    let total = env.output.get(rollup_key);

    let mut steps = Vec::new();
    steps.push(
        BreakdownStep::label("Hit DPS")
            .with_value(hit)
            .with_explain(format!("{hit:.0} hit-damage layer (MainSkillDPS)")),
    );
    steps.push(
        BreakdownStep::label(ailment_label)
            .with_value(ailment)
            .with_explain(format!("{ailment:.0} from {ailment_key}")),
    );
    steps.push(
        BreakdownStep::label(rollup_key)
            .with_value(total)
            .with_explain(format!("{hit:.0} + {ailment:.0} = {total:.0}")),
    );

    Breakdown {
        output_key: rollup_key.to_owned(),
        total,
        steps,
    }
}

/// Issue #34 follow-up: re-derive `ManaPerSecondCost`. PoB:
/// `mana_cost × cps` (perform.rs:4445). Two contributors — per-cast
/// cost and cast/swing rate. Spell builds tuning sustain want to
/// see whether their mana burn is driven by the cost or the speed.
fn mana_per_second_cost(env: &Env) -> Breakdown {
    let cost = env.output.get("MainSkillManaCost");
    let speed = env.output.get("MainSkillSpeed");
    let total = env.output.get("ManaPerSecondCost");

    let mut steps = Vec::new();
    steps.push(
        BreakdownStep::label("Mana cost")
            .with_value(cost)
            .with_explain(format!("{cost:.0} per cast (see MainSkillManaCost)")),
    );
    steps.push(
        BreakdownStep::label("Casts per second")
            .with_value(speed)
            .with_explain(format!("{speed:.2} cps from MainSkillSpeed")),
    );
    steps.push(
        BreakdownStep::label("ManaPerSecondCost")
            .with_value(total)
            .with_explain(format!("{cost:.0} × {speed:.2} = {total:.1}")),
    );

    Breakdown {
        output_key: "ManaPerSecondCost".to_owned(),
        total,
        steps,
    }
}

/// Issue #34 follow-up: re-derive `MainSkillManaCost`. PoB:
///
///   cost = base × (1 + ManaCost Inc / 100)
///
/// from `perform.rs:3219-3224`. The base cost comes from the gem's
/// per-level data and isn't stored separately on output, so the
/// helper back-derives it from the final cost and the Inc total
/// (`base = total / (1 + inc/100)`). The Inc step's value carries
/// the multiplier itself so the chain reads multiplicatively.
fn main_skill_mana_cost(env: &Env) -> Breakdown {
    let cfg = QueryCfg::default();
    let inc_total = env.mod_db.sum(ModType::Inc, &cfg, &env.state, "ManaCost");
    let mult = (1.0 + inc_total / 100.0).max(0.0);
    let total = env.output.get("MainSkillManaCost");
    let base = if mult > 1e-9 { total / mult } else { total };

    let mut steps = Vec::new();
    steps.push(
        BreakdownStep::label("Base mana cost")
            .with_value(base)
            .with_explain(format!("{base:.0} from the gem's per-level cost")),
    );
    let inc_mods: Vec<ModSource> = env
        .mod_db
        .iter_named("ManaCost")
        .filter(|m| m.kind == ModType::Inc)
        .map(ModSource::from_mod)
        .collect();
    steps.push(
        BreakdownStep::label("Increased / Reduced")
            .with_value(mult)
            .with_explain(if (inc_total - 0.0).abs() < 1e-9 {
                "no ManaCost mods (×1.000)".to_owned()
            } else {
                format!("{inc_total:+.0}% sum → ×{mult:.3}")
            })
            .with_sources(inc_mods),
    );
    steps.push(
        BreakdownStep::label("MainSkillManaCost")
            .with_value(total)
            .with_explain(format!("{base:.0} × {mult:.3} = {total:.0}")),
    );

    Breakdown {
        output_key: "MainSkillManaCost".to_owned(),
        total,
        steps,
    }
}

/// Issue #34 follow-up: re-derive `MainSkillAverageHitAfterAccuracy`.
/// Closes the damage chain — `AfterShock × HitChance/100`. Spells
/// always hit at 100% (the perform pass pins
/// `MainSkillHitChance = 100` for non-attack skills); attacks scale
/// by the formal hit chance computed against the enemy's evasion.
///
/// The hit-chance step's value carries the *factor* (0..=1) so the
/// chain reads multiplicatively across the resist / shock / accuracy
/// triple. The percent itself is in the explain text, with an
/// "always hits" hint at exactly 100%.
fn after_accuracy(env: &Env) -> Breakdown {
    let after_shock = env.output.get("MainSkillAverageHitAfterShock");
    let hit_pct = env.output.get("MainSkillHitChance");
    let hit_factor = (hit_pct / 100.0).clamp(0.0, 1.0);
    let total = env.output.get("MainSkillAverageHitAfterAccuracy");

    let mut steps = Vec::new();
    steps.push(
        BreakdownStep::label("After shock")
            .with_value(after_shock)
            .with_explain(format!("{after_shock:.0} from after-shock chain")),
    );
    steps.push(
        BreakdownStep::label("Hit chance")
            .with_value(hit_factor)
            .with_explain(if (hit_pct - 100.0).abs() < 1e-9 {
                "100% (always hits — spell or capped attack)".to_owned()
            } else {
                format!("{hit_pct:.0}% chance × {after_shock:.0}")
            }),
    );
    steps.push(
        BreakdownStep::label("After accuracy")
            .with_value(total)
            .with_explain(format!("{after_shock:.0} × {hit_factor:.3} = {total:.0}")),
    );

    Breakdown {
        output_key: "MainSkillAverageHitAfterAccuracy".to_owned(),
        total,
        steps,
    }
}

/// Issue #34 follow-up: re-derive `MainSkillAverageHitAfterShock`.
/// Chain:
///
///   AfterShock = AfterResist × ShockMult + PhysicalGainAsExtraDamage
///
/// Surfaces the source `AfterResist`, the `ShockMult` (= 1.0 for
/// shock-less builds), and the gain-as-extra layer when non-zero
/// (most spell builds and unmodded attacks omit it; phys-attack
/// builds with gain-as-elemental supports surface it). The total is
/// read directly from the stored output rather than recomputed so
/// the breakdown matches whatever rounding / clamps the perform
/// pass applied.
fn after_shock(env: &Env) -> Breakdown {
    let after_res = env.output.get("MainSkillAverageHitAfterResist");
    let shock_mult = env.output.get("MainSkillShockMult");
    let gain_as = env.output.get("PhysicalGainAsExtraDamage");
    let total = env.output.get("MainSkillAverageHitAfterShock");

    let mut steps = Vec::new();
    steps.push(
        BreakdownStep::label("After resist")
            .with_value(after_res)
            .with_explain(format!("{after_res:.0} from after-resist chain")),
    );
    steps.push(
        BreakdownStep::label("Shock multiplier")
            .with_value(shock_mult)
            .with_explain(if (shock_mult - 1.0).abs() < 1e-9 {
                "1.000 (no shock)".to_owned()
            } else {
                format!("{shock_mult:.3}× from active shock")
            }),
    );
    if gain_as > 0.0 {
        steps.push(
            BreakdownStep::label("Phys gain-as extra")
                .with_value(gain_as)
                .with_explain(format!("+{gain_as:.0} from PhysicalDamageGainAs<X>")),
        );
    }
    steps.push(
        BreakdownStep::label("After shock")
            .with_value(total)
            .with_explain(if gain_as > 0.0 {
                format!("{after_res:.0} × {shock_mult:.3} + {gain_as:.0} = {total:.0}")
            } else {
                format!("{after_res:.0} × {shock_mult:.3} = {total:.0}")
            }),
    );

    Breakdown {
        output_key: "MainSkillAverageHitAfterShock".to_owned(),
        total,
        steps,
    }
}

/// Issue #34 follow-up: re-derive `MainSkillAverageHitAfterResist`.
/// The output is `AverageHitWithCrit × (1 - effective_resist/100)`.
/// We don't recompute the after-resist value — we read it directly
/// from the output and surface the three components the user wants
/// to see: the source hit, the resist multiplier, and the final.
///
/// Falls back to `MainSkillAverageHit` (no-crit) when
/// `MainSkillAverageHitWithCrit` isn't set, mirroring the perform
/// pass's own fallback for spell-only builds.
fn after_resist(env: &Env) -> Breakdown {
    let avg = env
        .output
        .try_get("MainSkillAverageHitWithCrit")
        .unwrap_or_else(|| env.output.get("MainSkillAverageHit"));
    let eff_resist = env.output.get("MainSkillEnemyEffectiveResist");
    let res_factor = (1.0 - eff_resist / 100.0).max(0.0);
    let total = env.output.get("MainSkillAverageHitAfterResist");

    let mut steps = Vec::new();
    steps.push(
        BreakdownStep::label("Average hit (with crit)")
            .with_value(avg)
            .with_explain(format!("{avg:.0} pre-resist")),
    );
    steps.push(
        BreakdownStep::label("Enemy resist factor")
            .with_value(res_factor)
            .with_explain(format!("1 - {eff_resist:.0}% / 100 = {res_factor:.3}")),
    );
    steps.push(
        BreakdownStep::label("After resist")
            .with_value(total)
            .with_explain(format!("{avg:.0} × {res_factor:.3} = {total:.0}")),
    );

    Breakdown {
        output_key: "MainSkillAverageHitAfterResist".to_owned(),
        total,
        steps,
    }
}

/// Issue #34 follow-up: re-derive `MainSkillHitChance`. PoB's
/// formula:
///
///   raw = Accuracy / (Accuracy + (EnemyEvasion / 5)^0.9) × 125
///   chance = round(raw).clamp(5, 100)
///
/// from `perform.rs:3707-3712`. Spells (`is_attack = false`) skip
/// the formula entirely and pin `MainSkillHitChance = 100`. The
/// breakdown surfaces both modes:
/// - Attack: Accuracy / Enemy evasion / Hit chance steps.
/// - Spell: a single Hit chance step with an "always hits" hint so
///   the user doesn't go grinding accuracy on a spell build.
fn hit_chance_main_skill(env: &Env) -> Breakdown {
    let total = env.output.get("MainSkillHitChance");
    let mut steps = Vec::new();
    if (total - 100.0).abs() < 1e-9 && env.output.try_get("Accuracy").unwrap_or(0.0) == 0.0 {
        // Edge case: no accuracy stored at all (older test fixtures or
        // partial pipelines). Skip the attack steps and just emit the
        // always-hits hint so the breakdown is still meaningful.
        steps.push(
            BreakdownStep::label("Hit chance")
                .with_value(100.0)
                .with_explain("100% (always hits — spell or capped attack)".to_owned()),
        );
        return Breakdown {
            output_key: "MainSkillHitChance".to_owned(),
            total,
            steps,
        };
    }
    let acc = env.output.get("Accuracy");
    let evasion = env.output.get("EnemyEvasion");
    steps.push(
        BreakdownStep::label("Accuracy")
            .with_value(acc)
            .with_explain(format!("{acc:.0} from BASE + level + Dex (see Accuracy)")),
    );
    steps.push(
        BreakdownStep::label("Enemy evasion")
            .with_value(evasion)
            .with_explain(format!("{evasion:.0} from Config tab")),
    );
    steps.push(
        BreakdownStep::label("Hit chance")
            .with_value(total)
            .with_explain(if (total - 100.0).abs() < 1e-9 {
                "100% (always hits — spell or capped attack)".to_owned()
            } else {
                format!("round(Accuracy / (Accuracy + (EnemyEvasion/5)^0.9) × 125) = {total:.0}%")
            }),
    );
    Breakdown {
        output_key: "MainSkillHitChance".to_owned(),
        total,
        steps,
    }
}

/// Issue #34 follow-up: re-derive `Accuracy`. PoB computes
///
///   Accuracy = Σ BASE(Accuracy) + 2 × (level - 1) + 2 × Dex
///
/// from `perform_basic_stats:3699-3702`. We surface the three
/// contributors as discrete steps. The level term is back-derived from
/// `total - mod_acc - dex_term` — `Env` doesn't carry the character
/// level (the perform pass takes it as a parameter and folds it into
/// the output), so the helper recovers it from the difference rather
/// than threading a new field.
fn accuracy(env: &Env) -> Breakdown {
    let cfg = QueryCfg::default();
    let mod_acc = env.mod_db.sum(ModType::Base, &cfg, &env.state, "Accuracy");
    let dex = env.output.get("Dexterity");
    let dex_term = 2.0 * dex;
    let total = env.output.get("Accuracy");
    let level_term = (total - mod_acc - dex_term).max(0.0);

    let mut steps = Vec::new();
    let base_mods: Vec<ModSource> = env
        .mod_db
        .iter_named("Accuracy")
        .filter(|m| m.kind == ModType::Base)
        .map(ModSource::from_mod)
        .collect();
    steps.push(
        BreakdownStep::label("Base mods")
            .with_value(mod_acc)
            .with_explain(format!("+{mod_acc:.0} from BASE mods"))
            .with_sources(base_mods),
    );
    steps.push(
        BreakdownStep::label("Level")
            .with_value(level_term)
            .with_explain(format!("+{level_term:.0} from 2 × (character level - 1)")),
    );
    steps.push(
        BreakdownStep::label("Dexterity")
            .with_value(dex_term)
            .with_explain(format!("+{dex_term:.0} from 2 × {dex:.0} Dex")),
    );
    steps.push(
        BreakdownStep::label("Accuracy")
            .with_value(total)
            .with_explain(format!(
                "{mod_acc:.0} + {level_term:.0} + {dex_term:.0} = {total:.0}"
            )),
    );

    Breakdown {
        output_key: "Accuracy".to_owned(),
        total,
        steps,
    }
}

/// Issue #34 follow-up: re-derive `LifeRegen`. Mirrors
/// `perform_basic_stats:1742-1748`:
///
///   total = Σ BASE(LifeRegen) + Life × Σ BASE(LifeRegenPercent) / 100
///
/// The two compose linearly (no INC / MORE on the regen output
/// itself; rate-modifying mods land on the per-pool side via the Life
/// chain). Surfaced steps: flat regen mods, percent regen mods (with
/// the Life pool the percent multiplies), and the final sum.
fn life_regen(env: &Env) -> Breakdown {
    let cfg = QueryCfg::default();
    let flat = env.mod_db.sum(ModType::Base, &cfg, &env.state, "LifeRegen");
    let pct = env
        .mod_db
        .sum(ModType::Base, &cfg, &env.state, "LifeRegenPercent");
    let life = env.output.get("Life");
    let pct_contrib = life * pct / 100.0;
    let total = env.output.get("LifeRegen");

    let mut steps = Vec::new();
    let flat_mods: Vec<ModSource> = env
        .mod_db
        .iter_named("LifeRegen")
        .filter(|m| m.kind == ModType::Base)
        .map(ModSource::from_mod)
        .collect();
    steps.push(
        BreakdownStep::label("Flat regen")
            .with_value(flat)
            .with_explain(format!("+{flat:.1}/sec from LifeRegen BASE mods"))
            .with_sources(flat_mods),
    );

    if pct.abs() > 1e-9 {
        let pct_mods: Vec<ModSource> = env
            .mod_db
            .iter_named("LifeRegenPercent")
            .filter(|m| m.kind == ModType::Base)
            .map(ModSource::from_mod)
            .collect();
        steps.push(
            BreakdownStep::label("Percent regen")
                .with_value(pct_contrib)
                .with_explain(format!("{pct:.2}% × {life:.0} Life = {pct_contrib:.1}/sec"))
                .with_sources(pct_mods),
        );
    }

    steps.push(
        BreakdownStep::label("Life regen")
            .with_value(total)
            .with_explain(format!("{flat:.1} + {pct_contrib:.1} = {total:.1}/sec")),
    );

    Breakdown {
        output_key: "LifeRegen".to_owned(),
        total,
        steps,
    }
}

/// Issue #34 follow-up: re-derive `ManaRegen`. Mirrors
/// `perform_basic_stats:1750-1755`:
///
///   total = (Mana × 0.0175 + Σ BASE(ManaRegen)) × (1 + Σ INC(ManaRegen) / 100)
///
/// Different shape from Life regen: the baseline is 1.75% of max
/// Mana per second, then flat ManaRegen BASE mods stack additively,
/// then INC ManaRegen scales the whole thing. Surfaced steps:
/// baseline, flat adders, INC scaling, and the final product.
fn mana_regen(env: &Env) -> Breakdown {
    let cfg = QueryCfg::default();
    let mana = env.output.get("Mana");
    let baseline = mana * 0.0175;
    let flat = env.mod_db.sum(ModType::Base, &cfg, &env.state, "ManaRegen");
    let inc_total = env.mod_db.sum(ModType::Inc, &cfg, &env.state, "ManaRegen");
    let total = env.output.get("ManaRegen");

    let mut steps = Vec::new();
    steps.push(
        BreakdownStep::label("Baseline")
            .with_value(baseline)
            .with_explain(format!(
                "1.75% × {mana:.0} Mana = {baseline:.2}/sec (PoE constant)"
            )),
    );

    if flat.abs() > 1e-9 {
        let flat_mods: Vec<ModSource> = env
            .mod_db
            .iter_named("ManaRegen")
            .filter(|m| m.kind == ModType::Base)
            .map(ModSource::from_mod)
            .collect();
        steps.push(
            BreakdownStep::label("Flat regen")
                .with_value(flat)
                .with_explain(format!("+{flat:.1}/sec from ManaRegen BASE mods"))
                .with_sources(flat_mods),
        );
    }

    if inc_total.abs() > 1e-9 {
        let inc_mods: Vec<ModSource> = env
            .mod_db
            .iter_named("ManaRegen")
            .filter(|m| m.kind == ModType::Inc)
            .map(ModSource::from_mod)
            .collect();
        steps.push(
            BreakdownStep::label("Increased")
                .with_value(1.0 + inc_total / 100.0)
                .with_explain(format!("{inc_total:+.0}% sum"))
                .with_sources(inc_mods),
        );
    }

    steps.push(
        BreakdownStep::label("Mana regen")
            .with_value(total)
            .with_explain(format!(
                "({baseline:.2} + {flat:.1}) × (1 + {inc_total:.0}%) = {total:.1}/sec"
            )),
    );

    Breakdown {
        output_key: "ManaRegen".to_owned(),
        total,
        steps,
    }
}

/// Issue #34 follow-up: re-derive `EnergyShieldRegen`. ES regen has
/// no pool-tied baseline term — unlike Life (which has `LifeRegenPercent
/// × Life`) and Mana (which has the 1.75% × Mana baseline), ES regen
/// is purely `Σ BASE(EnergyShieldRegen) × (1 + Σ INC(EnergyShieldRegen) / 100)`
/// (per `perform_basic_stats`). With no flat or inc mods set, the
/// dispatch returns `None` so the Calcs panel falls back to the
/// generic contributing-modifiers view rather than rendering an
/// empty breakdown.
fn energy_shield_regen(env: &Env) -> Option<Breakdown> {
    let cfg = QueryCfg::default();
    let flat = env
        .mod_db
        .sum(ModType::Base, &cfg, &env.state, "EnergyShieldRegen");
    let inc_total = env
        .mod_db
        .sum(ModType::Inc, &cfg, &env.state, "EnergyShieldRegen");
    if flat.abs() < 1e-9 && inc_total.abs() < 1e-9 {
        return None;
    }
    let total = env.output.get("EnergyShieldRegen");

    let mut steps = Vec::new();
    if flat.abs() > 1e-9 {
        let flat_mods: Vec<ModSource> = env
            .mod_db
            .iter_named("EnergyShieldRegen")
            .filter(|m| m.kind == ModType::Base)
            .map(ModSource::from_mod)
            .collect();
        steps.push(
            BreakdownStep::label("Flat regen")
                .with_value(flat)
                .with_explain(format!("+{flat:.1}/sec from EnergyShieldRegen BASE mods"))
                .with_sources(flat_mods),
        );
    }

    if inc_total.abs() > 1e-9 {
        let inc_mods: Vec<ModSource> = env
            .mod_db
            .iter_named("EnergyShieldRegen")
            .filter(|m| m.kind == ModType::Inc)
            .map(ModSource::from_mod)
            .collect();
        steps.push(
            BreakdownStep::label("Increased")
                .with_value(1.0 + inc_total / 100.0)
                .with_explain(format!("{inc_total:+.0}% sum"))
                .with_sources(inc_mods),
        );
    }

    steps.push(
        BreakdownStep::label("Energy shield regen")
            .with_value(total)
            .with_explain(format!(
                "{flat:.1} × (1 + {inc_total:.0}%) = {total:.1}/sec"
            )),
    );

    Some(Breakdown {
        output_key: "EnergyShieldRegen".to_owned(),
        total,
        steps,
    })
}

/// Issue #34 follow-up: shared helper for the per-instance leech
/// caps and rates.
///
/// `Max{Life,Mana}LeechInstance` = `0.10 × pool` (per-instance cap)
/// `{Life,Mana}LeechInstanceRate` = `0.02 × pool` (per-instance rate)
///
/// Both share the Pool → Constant → Final shape; the caller supplies
/// the pool key, output key, step labels, the constant factor, and
/// the constant's explain string. Returns `None` when the pool is
/// zero.
#[allow(clippy::too_many_arguments)]
fn leech_instance(
    env: &Env,
    pool_key: &str,
    output_key: &str,
    factor_label: &str,
    final_label: &str,
    factor: f64,
    factor_explain: &str,
) -> Option<Breakdown> {
    let pool = env.output.get(pool_key);
    if pool.abs() < 1e-9 {
        return None;
    }
    let total = env.output.get(output_key);
    let pool_lower = pool_key.to_ascii_lowercase();

    let mut steps = Vec::new();
    steps.push(
        BreakdownStep::label(format!("{pool_key} pool"))
            .with_value(pool)
            .with_explain(format!("{pool:.0} max {pool_lower} from pool derivation")),
    );
    steps.push(
        BreakdownStep::label(factor_label)
            .with_value(factor)
            .with_explain(factor_explain.to_owned()),
    );
    steps.push(
        BreakdownStep::label(final_label)
            .with_value(total)
            .with_explain(format!("{factor:.2} × {pool:.0} = {total:.1}")),
    );

    Some(Breakdown {
        output_key: output_key.to_owned(),
        total,
        steps,
    })
}

/// Issue #34 follow-up: re-derive `MaxLifeLeechRate` /
/// `MaxManaLeechRate`. Both are `0.20 × pool` per
/// `perform_basic_stats` — PoE caps total leech at 20% of max pool
/// per second. Surfacing the chain makes the constant + pool source
/// explicit so users understand what their leech-rate ceiling is and
/// why pool-scaling matters for leech-driven sustain.
///
/// `pool_key` selects the pool ("Life" or "Mana"); the leech-rate
/// output is looked up by suffix. Returns `None` when the pool is
/// zero so the panel falls back to the generic mods view.
fn max_leech_rate(env: &Env, pool_key: &str) -> Option<Breakdown> {
    let pool = env.output.get(pool_key);
    if pool.abs() < 1e-9 {
        return None;
    }
    let rate = env.output.get(&format!("Max{pool_key}LeechRate"));
    let pool_lower = pool_key.to_ascii_lowercase();

    let mut steps = Vec::new();
    steps.push(
        BreakdownStep::label(format!("{pool_key} pool"))
            .with_value(pool)
            .with_explain(format!("{pool:.0} max {pool_lower} from pool derivation")),
    );
    steps.push(
        BreakdownStep::label("Cap")
            .with_value(0.20)
            .with_explain("20% of pool per second (PoE constant)".to_owned()),
    );
    steps.push(
        BreakdownStep::label(format!("Max {pool_lower} leech rate"))
            .with_value(rate)
            .with_explain(format!("0.20 × {pool:.0} = {rate:.1}/sec")),
    );

    Some(Breakdown {
        output_key: format!("Max{pool_key}LeechRate"),
        total: rate,
        steps,
    })
}

/// Issue #34 follow-up: re-derive `LowestOfMaximumLifeAndMaximumMana`.
/// PoB computes this as `life.min(mana)` in `perform_basic_stats`.
/// Surfacing the chain shows both pool sizes plus which one wins —
/// used by some unique-item / cluster-jewel mods that scale on the
/// smaller pool.
///
/// Tie-break: PoB's `f64::min` returns the first operand when equal,
/// so the chain prefers Life over Mana on ties. Returns `None` when
/// both pools are zero (no character loaded yet).
fn lowest_pool(env: &Env) -> Option<Breakdown> {
    let life = env.output.get("Life");
    let mana = env.output.get("Mana");
    if life.abs() < 1e-9 && mana.abs() < 1e-9 {
        return None;
    }
    let total = env.output.get("LowestOfMaximumLifeAndMaximumMana");
    let winner = if life <= mana { "Life" } else { "Mana" };

    let mut steps = Vec::new();
    steps.push(
        BreakdownStep::label("Life pool")
            .with_value(life)
            .with_explain(format!("{life:.0} max life from pool derivation")),
    );
    steps.push(
        BreakdownStep::label("Mana pool")
            .with_value(mana)
            .with_explain(format!("{mana:.0} max mana from pool derivation")),
    );
    steps.push(
        BreakdownStep::label("Lowest of life and mana")
            .with_value(total)
            .with_explain(format!("min({life:.0}, {mana:.0}) = {total:.0} ({winner})")),
    );

    Some(Breakdown {
        output_key: "LowestOfMaximumLifeAndMaximumMana".to_owned(),
        total,
        steps,
    })
}

/// Issue #34 follow-up: re-derive `LowestAttribute`. The output is
/// `min(Strength, Dexterity, Intelligence)` (PoB's `min().min()`
/// chain in `perform_basic_stats`). Surfacing the chain shows each
/// attribute's value plus the winner — used by `LowestAttribute`
/// trigger mods (e.g. unique boots, certain timeless jewels).
///
/// Tie-break: PoB's `f64::min` returns the first operand when equal,
/// and the chain is `Strength.min(Dexterity).min(Intelligence)`, so
/// the order of preference is Str → Dex → Int. The breakdown
/// surfaces whichever attribute is the source of the winning value.
fn lowest_attribute(env: &Env) -> Breakdown {
    let str_v = env.output.get("Strength");
    let dex_v = env.output.get("Dexterity");
    let int_v = env.output.get("Intelligence");
    let total = env.output.get("LowestAttribute");

    let mut steps = Vec::new();
    for (label, value) in [
        ("Strength", str_v),
        ("Dexterity", dex_v),
        ("Intelligence", int_v),
    ] {
        steps.push(
            BreakdownStep::label(label)
                .with_value(value)
                .with_explain(format!("{value:.0}")),
        );
    }
    // Pick winner via the same min-chain order PoB uses.
    let winner = if str_v <= dex_v && str_v <= int_v {
        "Strength"
    } else if dex_v <= int_v {
        "Dexterity"
    } else {
        "Intelligence"
    };
    steps.push(
        BreakdownStep::label("Lowest attribute")
            .with_value(total)
            .with_explain(format!(
                "min({str_v:.0}, {dex_v:.0}, {int_v:.0}) = {total:.0} ({winner})"
            )),
    );

    Breakdown {
        output_key: "LowestAttribute".to_owned(),
        total,
        steps,
    }
}

/// Issue #34 follow-up: re-derive `TotalAttr`. The output is just
/// `Strength + Dexterity + Intelligence`, but surfacing the chain
/// gives users the contributing attribute breakdown links and makes
/// the +N to all attributes story explicit. Each attribute step
/// links back to the per-attribute `pool_basic` derivation.
fn total_attributes(env: &Env) -> Breakdown {
    let str_v = env.output.get("Strength");
    let dex_v = env.output.get("Dexterity");
    let int_v = env.output.get("Intelligence");
    let total = env.output.get("TotalAttr");

    let mut steps = Vec::new();
    for (label, value) in [
        ("Strength", str_v),
        ("Dexterity", dex_v),
        ("Intelligence", int_v),
    ] {
        steps.push(
            BreakdownStep::label(label)
                .with_value(value)
                .with_explain(format!(
                    "+{value:.0} from the {label} pool — see its breakdown for sources"
                )),
        );
    }
    steps.push(
        BreakdownStep::label("Total attributes")
            .with_value(total)
            .with_explain(format!("{str_v:.0} + {dex_v:.0} + {int_v:.0} = {total:.0}")),
    );

    Breakdown {
        output_key: "TotalAttr".to_owned(),
        total,
        steps,
    }
}

/// Issue #34 follow-up: re-derive `LifeUnreserved` / `ManaUnreserved`.
/// PoB computes these as `pool − reserved` after `perform_reservations`
/// folds in flat + percent reservation contributions from auras /
/// heralds. The breakdown surfaces the arithmetic — Pool, Reserved
/// (with the percent it represents), and the final Unreserved — so
/// reservation builds can see exactly how much each aura ate into
/// the pool.
///
/// `pool_key` is the underlying pool stat ("Life" or "Mana"); the
/// reserved / unreserved / percent outputs are looked up by suffix.
/// Returns `None` when the pool is zero (no character loaded yet, or
/// a pure-CI build for Life) so the panel falls back to the generic
/// mods view.
fn unreserved_pool(env: &Env, pool_key: &str) -> Option<Breakdown> {
    let pool = env.output.get(pool_key);
    if pool.abs() < 1e-9 {
        return None;
    }
    let reserved = env.output.get(&format!("{pool_key}Reserved"));
    let reserved_pct = env.output.get(&format!("{pool_key}ReservedPercent"));
    let unreserved = env.output.get(&format!("{pool_key}Unreserved"));
    let unreserved_pct = env.output.get(&format!("{pool_key}UnreservedPercent"));
    let pool_lower = pool_key.to_ascii_lowercase();

    let mut steps = Vec::new();
    steps.push(
        BreakdownStep::label(format!("{pool_key} pool"))
            .with_value(pool)
            .with_explain(format!("{pool:.0} max {pool_lower} from pool derivation")),
    );
    steps.push(
        BreakdownStep::label("Reserved")
            .with_value(reserved)
            .with_explain(format!(
                "−{reserved:.0} ({reserved_pct:.1}%) — see auras / heralds for source mods"
            )),
    );
    steps.push(
        BreakdownStep::label("Unreserved")
            .with_value(unreserved)
            .with_explain(format!(
                "{pool:.0} − {reserved:.0} = {unreserved:.0} ({unreserved_pct:.1}% of pool)"
            )),
    );

    Some(Breakdown {
        output_key: format!("{pool_key}Unreserved"),
        total: unreserved,
        steps,
    })
}

/// Issue #34 follow-up: shared helper for the three Life-pool
/// alias outputs (`LifeHitPool`, `LifeRecoverable`, `StunThreshold`).
/// All three collapse to the `Life` output in Phase 2 (no recoup
/// mods, no MoM split); the breakdown calls out the alias so users
/// don't wonder why three rows on the defence panel show the same
/// number.
///
/// Returns `None` when Life is zero (no character loaded yet).
fn life_alias(env: &Env, output_key: &str, final_label: &str) -> Option<Breakdown> {
    let life = env.output.get("Life");
    if life.abs() < 1e-9 {
        return None;
    }
    let total = env.output.get(output_key);

    let mut steps = Vec::new();
    steps.push(
        BreakdownStep::label("Life")
            .with_value(life)
            .with_explain(format!(
                "{life:.0} from the Life pool — see its breakdown for the source mods"
            )),
    );
    steps.push(
        BreakdownStep::label(final_label)
            .with_value(total)
            .with_explain(format!(
                "{total:.0} — alias of Life (Phase 2 baseline; no recoup / MoM split yet)"
            )),
    );

    Some(Breakdown {
        output_key: output_key.to_owned(),
        total,
        steps,
    })
}

/// Issue #34 follow-up: shared helper for the per-element
/// `<Element>TotalHitPool` / `<Element>TotalPool` outputs. PoB
/// exposes both for each of the five damage types (Physical / Fire
/// / Cold / Lightning / Chaos); in Phase 2 they all collapse to
/// `life + es + ward` (no MoM yet, no per-element split). Surfacing
/// the chain shows the additive components.
///
/// `elem` selects the canonical element name; `suffix` selects
/// `"TotalHitPool"` or `"TotalPool"`; `final_label` is the user-
/// visible row name.
///
/// Returns `None` when the underlying pool is zero.
fn total_pool(env: &Env, elem: &str, suffix: &str, final_label: &str) -> Option<Breakdown> {
    let life = env.output.get("Life");
    let es = env.output.get("EnergyShield");
    let ward = env.output.get("Ward");
    let pool = life + es + ward;
    if pool.abs() < 1e-9 {
        return None;
    }
    let total = env.output.get(&format!("{elem}{suffix}"));

    let mut steps = Vec::new();
    steps.push(
        BreakdownStep::label("Life")
            .with_value(life)
            .with_explain(format!("{life:.0} from the Life pool")),
    );
    steps.push(
        BreakdownStep::label("Energy shield")
            .with_value(es)
            .with_explain(format!("{es:.0} from the EnergyShield pool")),
    );
    steps.push(
        BreakdownStep::label("Ward")
            .with_value(ward)
            .with_explain(format!("{ward:.0} from the Ward pool")),
    );
    steps.push(
        BreakdownStep::label(final_label)
            .with_value(total)
            .with_explain(format!(
                "{life:.0} + {es:.0} + {ward:.0} = {total:.0} ({elem} pool, Phase 2 single-pool baseline)"
            )),
    );

    Some(Breakdown {
        output_key: format!("{elem}{suffix}"),
        total,
        steps,
    })
}

/// Issue #34 follow-up: shared helper for the five
/// `<Element>DotEHP` outputs. PoB derives all five through the same
/// `perform_ehp` shape:
///
///   dot_ehp = pool / (1 - resist)
///
/// — DoT damage doesn't go through hit-time defences (block,
/// suppression), so the taken multiplier is just the resist factor.
/// For `Physical` the source value is `PhysicalDamageReduction`
/// rather than a resist; everything else uses `<Element>ResistTotal`.
///
/// Returns `None` when the pool is zero.
fn dot_ehp(env: &Env, elem: &str) -> Option<Breakdown> {
    let life = env.output.get("Life");
    let es = env.output.get("EnergyShield");
    let ward = env.output.get("Ward");
    let pool = life + es + ward;
    if pool.abs() < 1e-9 {
        return None;
    }
    let total = env.output.get(&format!("{elem}DotEHP"));
    // Back-derive the taken multiplier from pool / dot_ehp so the
    // breakdown stays correct regardless of which mitigation source
    // populates it (resist for elements, reduction for physical).
    let taken = if total > 1e-9 { pool / total } else { 1.0 };
    let mitigation_pct = (1.0 - taken) * 100.0;
    let mitigation_label = if elem == "Physical" {
        "physical damage reduction"
    } else {
        "resistance"
    };

    let mut steps = Vec::new();
    steps.push(
        BreakdownStep::label("Pool")
            .with_value(pool)
            .with_explain(format!(
                "{life:.0} life + {es:.0} ES + {ward:.0} ward = {pool:.0}"
            )),
    );
    steps.push(
        BreakdownStep::label("Taken multiplier")
            .with_value(taken)
            .with_explain(format!(
                "1 - {mitigation_pct:.0}% {mitigation_label} = {taken:.2}× damage taken"
            )),
    );
    steps.push(
        BreakdownStep::label(format!("{elem} DoT EHP"))
            .with_value(total)
            .with_explain(format!("{pool:.0} / {taken:.2} = {total:.0}")),
    );

    Some(Breakdown {
        output_key: format!("{elem}DotEHP"),
        total,
        steps,
    })
}

/// Issue #34 follow-up: re-derive `MainSkillEnemyEffectiveResist`.
/// PoB derives the post-penetration enemy resist value used by the
/// damage chain as `(enemy_resist_raw - elem_pen).clamp(-200, 95)`.
/// The configured enemy resist (boss preset on
/// `Character::config`) and the `<Element>Penetration` BASE-mod sum
/// aren't both stored as output keys, so a step-by-step
/// decomposition isn't possible without that wiring. Surfacing the
/// final value plus the cap rationale lets users see why a build
/// with 100% pen against 75% resist still nets at -25% (not lower).
///
/// Returns `None` when the resist is zero (no skill loaded, or
/// non-elemental hit).
fn main_skill_enemy_effective_resist(env: &Env) -> Option<Breakdown> {
    let total = env.output.get("MainSkillEnemyEffectiveResist");
    if total.abs() < 1e-9 {
        return None;
    }

    let mut steps = Vec::new();
    steps.push(
        BreakdownStep::label("Cap range")
            .with_value(0.0)
            .with_explain("[-200, 95]% — PoB clamp on (enemy_resist - penetration)".to_owned()),
    );
    steps.push(
        BreakdownStep::label("Effective enemy resist")
            .with_value(total)
            .with_explain(format!(
                "{total:.0}% from (enemy raw resist - <Element>Penetration BASE) clamp([-200, 95]) — enemy resist comes from the configured boss preset"
            )),
    );

    Some(Breakdown {
        output_key: "MainSkillEnemyEffectiveResist".to_owned(),
        total,
        steps,
    })
}

/// Issue #34 follow-up: shared helper for the per-element
/// `<Element>{Min,Max}` outputs. PoB derives them through the same
/// `base × mult` chain as `MainSkillHit{Min,Max}` but with the
/// per-element `<Element>{Min,Max}Base` raw skill values.
///
/// Returns `None` when the element wasn't the active skill's damage
/// type (no per-element outputs populated).
fn element_hit_bound(env: &Env, elem: &str, bound: &str) -> Option<Breakdown> {
    let base = env.output.get(&format!("{elem}{bound}Base"));
    if base.abs() < 1e-9 {
        return None;
    }
    let total = env.output.get(&format!("{elem}{bound}"));
    let mult = if base > 1e-9 { total / base } else { 1.0 };
    let bound_lower = bound.to_ascii_lowercase();

    let mut steps = Vec::new();
    steps.push(
        BreakdownStep::label(format!("Base {bound_lower}"))
            .with_value(base)
            .with_explain(format!(
                "{base:.0} from {elem}{bound}Base — raw per-level value before player mods"
            )),
    );
    steps.push(
        BreakdownStep::label("Multiplier")
            .with_value(mult)
            .with_explain(format!(
                "{mult:.2}× from (1 + INC) × MORE × (1 + quality/200) — see MainSkillAverageHit for the per-step decomposition"
            )),
    );
    steps.push(
        BreakdownStep::label(format!("{elem} {bound_lower}"))
            .with_value(total)
            .with_explain(format!("{base:.0} × {mult:.2} = {total:.1}")),
    );

    Some(Breakdown {
        output_key: format!("{elem}{bound}"),
        total,
        steps,
    })
}

/// Issue #34 follow-up: shared helper for the per-element
/// `<Element>{Min,Max}Base` raw skill outputs. Mirror of
/// `main_skill_base_bound` but per damage type — single-row
/// breakdown surfacing the raw skill data with a level/quality
/// hint so the click-through chain stays consistent.
///
/// Returns `None` when the element wasn't the active skill's
/// damage type.
fn element_base_bound(env: &Env, elem: &str, bound: &str) -> Option<Breakdown> {
    let total = env.output.get(&format!("{elem}{bound}Base"));
    if total.abs() < 1e-9 {
        return None;
    }
    let bound_lower = bound.to_ascii_lowercase();

    let mut steps = Vec::new();
    steps.push(
        BreakdownStep::label(format!("{elem} {bound_lower} base"))
            .with_value(total)
            .with_explain(format!(
                "{total:.0} from skill stats — raw per-level {elem} {bound_lower} value (level / quality scaling already folded in)"
            )),
    );

    Some(Breakdown {
        output_key: format!("{elem}{bound}Base"),
        total,
        steps,
    })
}

/// Issue #34 follow-up: shared helper for the five
/// `<Element>HitAverage` outputs (Fire / Cold / Lightning /
/// Physical / Chaos). PoB derives them as
/// `(<Element>Min + <Element>Max) / 2` per `perform_skill_dps` —
/// post-mod hit values for that damage type. Surfaces the bounds
/// the average is computed from so users can see the range
/// without click-through.
///
/// Returns `None` when the element wasn't the active skill's
/// damage type (no per-element outputs populated).
fn element_hit_average(env: &Env, elem: &str) -> Option<Breakdown> {
    let total = env.output.get(&format!("{elem}HitAverage"));
    if total.abs() < 1e-9 {
        return None;
    }
    let min = env.output.get(&format!("{elem}Min"));
    let max = env.output.get(&format!("{elem}Max"));
    let elem_lower = elem.to_ascii_lowercase();

    let mut steps = Vec::new();
    steps.push(
        BreakdownStep::label("Min")
            .with_value(min)
            .with_explain(format!(
                "{min:.0} from {elem}Min — see its breakdown for the base × multiplier chain"
            )),
    );
    steps.push(
        BreakdownStep::label("Max")
            .with_value(max)
            .with_explain(format!(
                "{max:.0} from {elem}Max — see its breakdown for the base × multiplier chain"
            )),
    );
    steps.push(
        BreakdownStep::label(format!("{elem} hit average"))
            .with_value(total)
            .with_explain(format!(
                "({min:.0} + {max:.0}) / 2 = {total:.1} {elem_lower} damage per hit"
            )),
    );

    Some(Breakdown {
        output_key: format!("{elem}HitAverage"),
        total,
        steps,
    })
}

/// Issue #34 follow-up: shared helper for `LifeReserved` /
/// `ManaReserved`. Complementary to the `unreserved_pool` helper —
/// PoB derives Reserved as `pool - unreserved` after
/// `perform_reservations` folds in flat + percent contributions
/// from active auras / heralds. The flat / percent breakdown
/// happens inside the aura-sweep pass and isn't both stored on
/// `env.output`, so the breakdown surfaces the high-level
/// subtraction with a back-link to the auras / heralds.
///
/// Returns `None` when the pool is zero.
fn reserved_pool(env: &Env, pool_key: &str) -> Option<Breakdown> {
    let pool = env.output.get(pool_key);
    if pool.abs() < 1e-9 {
        return None;
    }
    let unreserved = env.output.get(&format!("{pool_key}Unreserved"));
    let reserved = env.output.get(&format!("{pool_key}Reserved"));
    let reserved_pct = env.output.get(&format!("{pool_key}ReservedPercent"));
    let pool_lower = pool_key.to_ascii_lowercase();

    let mut steps = Vec::new();
    steps.push(
        BreakdownStep::label(format!("{pool_key} pool"))
            .with_value(pool)
            .with_explain(format!("{pool:.0} max {pool_lower} from pool derivation")),
    );
    steps.push(
        BreakdownStep::label("Unreserved")
            .with_value(unreserved)
            .with_explain(format!(
            "{unreserved:.0} from {pool_key}Unreserved — see its breakdown for the auras / heralds"
        )),
    );
    steps.push(
        BreakdownStep::label(format!("{pool_key} reserved"))
            .with_value(reserved)
            .with_explain(format!(
                "{pool:.0} − {unreserved:.0} = {reserved:.0} ({reserved_pct:.1}% of pool)"
            )),
    );

    Some(Breakdown {
        output_key: format!("{pool_key}Reserved"),
        total: reserved,
        steps,
    })
}

/// Issue #34 follow-up: shared helper for the four reservation-percent
/// outputs (Life/Mana × Reserved/Unreserved). PoB derives each as
/// `<absolute> / pool × 100` after `perform_reservations` folds in
/// the flat + percent reservation contributions from auras / heralds
/// (the absolute is exposed as `<Pool><Reserved|Unreserved>` and the
/// percent as `<Pool><Reserved|Unreserved>Percent`).
///
/// Surfaces Pool, the absolute reserved/unreserved value (with a
/// back-link to the matching Life/Mana[Un]reserved row), and the
/// percentage formula.
///
/// `pool_key` selects "Life" / "Mana"; `kind` selects "Reserved" /
/// "Unreserved". Returns `None` when the pool is zero.
fn reservation_percent(env: &Env, pool_key: &str, kind: &str) -> Option<Breakdown> {
    let pool = env.output.get(pool_key);
    if pool.abs() < 1e-9 {
        return None;
    }
    let absolute_key = format!("{pool_key}{kind}");
    let percent_key = format!("{absolute_key}Percent");
    let absolute = env.output.get(&absolute_key);
    let percent = env.output.get(&percent_key);
    let pool_lower = pool_key.to_ascii_lowercase();
    let kind_lower = kind.to_ascii_lowercase();

    let mut steps = Vec::new();
    steps.push(
        BreakdownStep::label(format!("{pool_key} pool"))
            .with_value(pool)
            .with_explain(format!("{pool:.0} max {pool_lower} from pool derivation")),
    );
    steps.push(
        BreakdownStep::label(kind)
            .with_value(absolute)
            .with_explain(format!(
                "{absolute:.0} from {absolute_key} — see its breakdown for the auras / heralds eating the pool"
            )),
    );
    steps.push(
        BreakdownStep::label(format!("{pool_key} {kind_lower} (%)"))
            .with_value(percent)
            .with_explain(format!("{absolute:.0} / {pool:.0} × 100 = {percent:.1}%")),
    );

    Some(Breakdown {
        output_key: percent_key,
        total: percent,
        steps,
    })
}

/// Issue #34 follow-up: re-derive `AoERadius` (the base AoE radius
/// before the area-mod sqrt scaling). PoB derives it as
/// `skill_base + Σ BASE("AreaOfEffect")` (`perform_skill_dps`).
/// Surfaces the BASE-mod sum as a separate row so users can see
/// what's adding to the gem's intrinsic radius before the
/// AreaOfEffectMod / sqrt scaling expands it.
///
/// `skill_base` isn't stored separately on env.output, so the
/// `Base radius` step is back-derived as `total - BASE-sum` with
/// a "from skill stats + BASE adders" final-step explain.
///
/// Returns `None` when the AoE radius is zero (non-AoE skill, or
/// no skill loaded).
fn aoe_radius_base(env: &Env) -> Option<Breakdown> {
    let total = env.output.get("AoERadius");
    if total.abs() < 1e-9 {
        return None;
    }
    let cfg = QueryCfg::default();
    let base_sum = env
        .mod_db
        .sum(ModType::Base, &cfg, &env.state, "AreaOfEffect");
    let skill_base = total - base_sum;

    let mut steps = Vec::new();

    if base_sum.abs() > 1e-9 {
        let base_mods: Vec<ModSource> = env
            .mod_db
            .iter_named("AreaOfEffect")
            .filter(|m| m.kind == ModType::Base)
            .map(ModSource::from_mod)
            .collect();
        steps.push(
            BreakdownStep::label("BASE adders")
                .with_value(base_sum)
                .with_explain(format!(
                    "+{base_sum:.0} from AreaOfEffect BASE mods (passives / gear)"
                ))
                .with_sources(base_mods),
        );
    }

    steps.push(
        BreakdownStep::label("Base radius")
            .with_value(total)
            .with_explain(format!(
                "{skill_base:.0} from skill stats + {base_sum:.0} BASE adders = {total:.0} (feeds AreaOfEffectRadius via the sqrt scaling)"
            )),
    );

    Some(Breakdown {
        output_key: "AoERadius".to_owned(),
        total,
        steps,
    })
}

/// Issue #34 follow-up: shared helper for `LifeFlaskRecovery` /
/// `ManaFlaskRecovery`. PoB takes the `max` across the five flask
/// slots' per-flask `Flask<N><Pool>Recovery` values because the
/// user only fires one flask at a time. The Calcs tab surfaced
/// the max as a single number; users couldn't tell which flask
/// was contributing or how the slots compared.
///
/// Surfaced steps walk all five flask slots (zeros for empty
/// slots, with their Flask<N><Pool>Recovery value when present)
/// → Final "<Pool> flask recovery" line whose explain calls out
/// the source flask.
///
/// `pool` is `"Life"` or `"Mana"`; the per-slot and final keys
/// follow `Flask<N><Pool>Recovery` / `<Pool>FlaskRecovery`. Returns
/// `None` when no flask of that type is equipped (perform.rs
/// doesn't write the final key in that case).
fn flask_recovery(env: &Env, pool: &str) -> Option<Breakdown> {
    let final_key = format!("{pool}FlaskRecovery");
    let total = env.output.get(&final_key);
    if total.abs() < 1e-9 {
        return None;
    }

    let mut steps = Vec::new();
    let mut winner: Option<usize> = None;
    let mut winner_value = f64::NEG_INFINITY;
    for slot in 1..=5 {
        let value = env.output.get(&format!("Flask{slot}{pool}Recovery"));
        if value > winner_value {
            winner_value = value;
            winner = Some(slot);
        }
        steps.push(
            BreakdownStep::label(format!("Flask {slot}"))
                .with_value(value)
                .with_explain(if value > 0.0 {
                    format!("{value:.0} from Flask{slot}{pool}Recovery")
                } else {
                    "(empty / no recovery on this flask)".to_owned()
                }),
        );
    }

    let pool_lower = pool.to_ascii_lowercase();
    let winner_str = winner
        .map(|s| format!("Flask {s}"))
        .unwrap_or_else(|| "—".to_owned());
    steps.push(
        BreakdownStep::label(format!("{pool} flask recovery"))
            .with_value(total)
            .with_explain(format!(
                "max(slot 1..5) = {total:.0} {pool_lower} (from {winner_str})"
            )),
    );

    Some(Breakdown {
        output_key: final_key,
        total,
        steps,
    })
}

/// Issue #34 follow-up: re-derive `EnemyPhysReduction`. PoB
/// computes it as `armour / (armour + 5 × raw)` capped at 90%
/// (the `EnemyPhysicalDamageReductionCap` constant), where:
///   - `armour` is the configured enemy armour (boss preset)
///   - `raw` is the player's per-hit physical average BEFORE crit
///     averaging
///
/// The breakdown surfaces the raw hit value (back-derivable from
/// `MainSkillAverageHit`) and the cap. The configured enemy armour
/// is on `Character::config` and not on `env.output`, so the explain
/// notes that the soak depends on the enemy preset rather than
/// trying to back-derive armour from a single equation.
///
/// Returns `None` when the reduction is zero (non-physical hit, or
/// no enemy armour).
fn enemy_phys_reduction(env: &Env) -> Option<Breakdown> {
    let total = env.output.get("EnemyPhysReduction");
    if total.abs() < 1e-9 {
        return None;
    }
    let raw = env.output.get("MainSkillAverageHit");

    let mut steps = Vec::new();
    steps.push(
        BreakdownStep::label("Raw hit")
            .with_value(raw)
            .with_explain(format!(
                "{raw:.0} per-hit damage from MainSkillAverageHit (drives the armour soak)"
            )),
    );
    steps.push(BreakdownStep::label("Cap").with_value(90.0).with_explain(
        "90% — DamageReductionMax PoE constant (caps the armour formula)".to_owned(),
    ));
    steps.push(
        BreakdownStep::label("Enemy physical damage reduction")
            .with_value(total)
            .with_explain(format!(
                "min(armour / (armour + 5 × {raw:.0}), 90%) = {total:.1}% (armour from configured enemy preset)"
            )),
    );

    Some(Breakdown {
        output_key: "EnemyPhysReduction".to_owned(),
        total,
        steps,
    })
}

/// Issue #34 follow-up: re-derive `MainSkillLevel`. PoB clamps the
/// active gem's level into `[1, 40]` (`perform_skill_dps`'s `let
/// gem_level = main.level.clamp(1, 40)`). Calcs panel rows that
/// depend on level (base damage, mana cost, area) link back to
/// this row. Surfacing the value as a single-step breakdown keeps
/// the click-through chain consistent.
///
/// Returns `None` when no skill is loaded (level 0).
fn main_skill_level(env: &Env) -> Option<Breakdown> {
    let total = env.output.get("MainSkillLevel");
    if total.abs() < 1e-9 {
        return None;
    }

    let mut steps = Vec::new();
    steps.push(
        BreakdownStep::label("Main skill level")
            .with_value(total)
            .with_explain(format!(
                "{total:.0} from the active gem (clamped to [1, 40] in perform_skill_dps)"
            )),
    );

    Some(Breakdown {
        output_key: "MainSkillLevel".to_owned(),
        total,
        steps,
    })
}

/// Issue #34 follow-up: re-derive `CastRate`. `perform_skill_dps`
/// sets `env.output.set("CastRate", cps)` (perform.rs ~line 4320)
/// where `cps` is the same per-second rate already exposed as
/// `MainSkillSpeed` (the cast/attack-speed chain output). For
/// skills that use cast-time terminology the Calcs panel reads
/// CastRate; surfacing it as a single-step breakdown keeps the
/// click-through chain consistent without duplicating the speed
/// computation.
///
/// Returns `None` when no skill is loaded (CastRate = 0).
fn cast_rate(env: &Env) -> Option<Breakdown> {
    let total = env.output.get("CastRate");
    if total.abs() < 1e-9 {
        return None;
    }

    let mut steps = Vec::new();
    steps.push(
        BreakdownStep::label("Cast rate")
            .with_value(total)
            .with_explain(format!(
                "{total:.2}/s — alias of MainSkillSpeed for skills using cast-time terminology (set in perform_skill_dps)"
            )),
    );

    Some(Breakdown {
        output_key: "CastRate".to_owned(),
        total,
        steps,
    })
}

/// Issue #34 follow-up: re-derive `WeaponRangeMetre`. PoB exposes
/// `WeaponRange` in engine units and `WeaponRangeMetre` as `units /
/// 10` for the Calcs panel — same conversion shape as
/// `AreaOfEffectRadiusMetres`. Surfacing the chain lets users see
/// the engine-unit value next to the metres value without
/// context-switching to WeaponRange.
///
/// Returns `None` when WeaponRange is zero (no character loaded yet).
fn weapon_range_metre(env: &Env) -> Option<Breakdown> {
    let units = env.output.get("WeaponRange");
    if units.abs() < 1e-9 {
        return None;
    }
    let metres = env.output.get("WeaponRangeMetre");

    let mut steps = Vec::new();
    steps.push(
        BreakdownStep::label("Range (engine units)")
            .with_value(units)
            .with_explain(format!(
                "{units:.0} from WeaponRange — see its breakdown for the weapon-data source"
            )),
    );
    steps.push(
        BreakdownStep::label("Weapon range (metres)")
            .with_value(metres)
            .with_explain(format!("{units:.0} / 10 = {metres:.2} m")),
    );

    Some(Breakdown {
        output_key: "WeaponRangeMetre".to_owned(),
        total: metres,
        steps,
    })
}

/// Issue #34 follow-up: shared helper for the uncapped `<Element>Resist`
/// outputs (Fire / Cold / Lightning). PoB exposes both the uncapped
/// raw resist (`FireResist`) and the capped value (`FireResistTotal`)
/// — the uncapped row reveals the user's overcap, e.g. a 90% sum
/// capped at 75% means 15% wasted resist.
///
/// Walks BASE + ElementalResist umbrella + level penalty → Final
/// without the cap clamp (that's a separate row exposed via the
/// existing `elemental_resist` helper).
///
/// Returns `None` when the resist is zero (no character loaded yet).
fn uncapped_elemental_resist(env: &Env, elem: &str) -> Option<Breakdown> {
    let total = env.output.get(&format!("{elem}Resist"));
    if total.abs() < 1e-9 {
        return None;
    }
    let cfg = QueryCfg::default();
    let resist_key = format!("{elem}Resist");
    let elem_base = env.mod_db.sum(ModType::Base, &cfg, &env.state, &resist_key);
    let umbrella = env
        .mod_db
        .sum(ModType::Base, &cfg, &env.state, "ElementalResist");
    let level_penalty = total - elem_base - umbrella;

    let mut steps = Vec::new();
    let elem_mods: Vec<ModSource> = env
        .mod_db
        .iter_named(&resist_key)
        .filter(|m| m.kind == ModType::Base)
        .map(ModSource::from_mod)
        .collect();
    steps.push(
        BreakdownStep::label(format!("{elem} Resist BASE"))
            .with_value(elem_base)
            .with_explain(format!("+{elem_base:.0}% from {elem}Resist BASE mods"))
            .with_sources(elem_mods),
    );

    if umbrella.abs() > 1e-9 {
        let umbrella_mods: Vec<ModSource> = env
            .mod_db
            .iter_named("ElementalResist")
            .filter(|m| m.kind == ModType::Base)
            .map(ModSource::from_mod)
            .collect();
        steps.push(
            BreakdownStep::label("Elemental Resist BASE")
                .with_value(umbrella)
                .with_explain(format!("+{umbrella:.0}% umbrella adders"))
                .with_sources(umbrella_mods),
        );
    }

    if level_penalty.abs() > 1e-9 {
        steps.push(
            BreakdownStep::label("Level penalty")
                .with_value(level_penalty)
                .with_explain(
                    "Act 5 / Act 10 story penalty (-30 / -60 by post-act level threshold)"
                        .to_owned(),
                ),
        );
    }

    steps.push(
        BreakdownStep::label(format!("{elem} Resist (uncapped)"))
            .with_value(total)
            .with_explain(format!(
                "{elem_base:.0} + {umbrella:.0} + {level_penalty:.0} = {total:.0}% (uncapped — see {elem}ResistTotal for the capped value)"
            )),
    );

    Some(Breakdown {
        output_key: format!("{elem}Resist"),
        total,
        steps,
    })
}

/// Issue #34 follow-up: ChaosResist mirror of
/// `uncapped_elemental_resist` without the umbrella step (chaos
/// doesn't pick up `ElementalResist` adders).
fn uncapped_chaos_resist(env: &Env) -> Option<Breakdown> {
    let total = env.output.get("ChaosResist");
    if total.abs() < 1e-9 {
        return None;
    }
    let cfg = QueryCfg::default();
    let chaos_base = env
        .mod_db
        .sum(ModType::Base, &cfg, &env.state, "ChaosResist");
    let level_penalty = total - chaos_base;

    let mut steps = Vec::new();
    let chaos_mods: Vec<ModSource> = env
        .mod_db
        .iter_named("ChaosResist")
        .filter(|m| m.kind == ModType::Base)
        .map(ModSource::from_mod)
        .collect();
    steps.push(
        BreakdownStep::label("Chaos Resist BASE")
            .with_value(chaos_base)
            .with_explain(format!("+{chaos_base:.0}% from ChaosResist BASE mods"))
            .with_sources(chaos_mods),
    );

    if level_penalty.abs() > 1e-9 {
        steps.push(
            BreakdownStep::label("Level penalty")
                .with_value(level_penalty)
                .with_explain(
                    "Act 5 / Act 10 story penalty (-30 / -60 by post-act level threshold)"
                        .to_owned(),
                ),
        );
    }

    steps.push(
        BreakdownStep::label("Chaos Resist (uncapped)")
            .with_value(total)
            .with_explain(format!(
                "{chaos_base:.0} + {level_penalty:.0} = {total:.0}% (uncapped — see ChaosResistTotal for the capped value)"
            )),
    );

    Some(Breakdown {
        output_key: "ChaosResist".to_owned(),
        total,
        steps,
    })
}

/// Issue #34 follow-up: shared helper for `MainSkillBaseMin` /
/// `MainSkillBaseMax`. Surfaces the raw per-level base damage value
/// from skill stats — what the gem provides before player mods. The
/// breakdown is intentionally a single row since the value is raw
/// skill data and not derived from anything we can decompose
/// further; the click-through chain stays consistent (every Calcs
/// row gets a breakdown).
///
/// Returns `None` when no skill is loaded.
fn main_skill_base_bound(env: &Env, bound: &str) -> Option<Breakdown> {
    let total = env.output.get(&format!("MainSkillBase{bound}"));
    if total.abs() < 1e-9 {
        return None;
    }
    let bound_lower = bound.to_ascii_lowercase();

    let mut steps = Vec::new();
    steps.push(
        BreakdownStep::label(format!("Base {bound_lower}"))
            .with_value(total)
            .with_explain(format!(
                "{total:.0} from skill stats — raw per-level value before player mods (level / quality scaling already folded in)"
            )),
    );

    Some(Breakdown {
        output_key: format!("MainSkillBase{bound}"),
        total,
        steps,
    })
}

/// Issue #34 follow-up: shared helper for `MainSkillHitMin` /
/// `MainSkillHitMax`. PoB derives both bounds via the same chain in
/// `perform_skill_dps`:
///
///   hit_<bound> = base_<bound> × (1 + Σ INC(Damage*) / 100) × Π MORE(Damage*) × (1 + quality/200)
///
/// The combined multiplier collapses to `hit_<bound> / base_<bound>`,
/// so the breakdown back-derives it from the stored values. Surfaces
/// the raw skill-base value alongside the composed mult so users can
/// see what their increased / more / quality stack contributes; for
/// the per-step decomposition see the MainSkillAverageHit breakdown.
///
/// `bound` is `"Min"` or `"Max"`; the relevant base / final outputs
/// are looked up by suffix. Returns `None` when no skill is loaded
/// (no MainSkill base values populated).
fn main_skill_hit_bound(env: &Env, bound: &str) -> Option<Breakdown> {
    let base = env.output.get(&format!("MainSkillBase{bound}"));
    if base.abs() < 1e-9 {
        return None;
    }
    let total = env.output.get(&format!("MainSkillHit{bound}"));
    let mult = if base > 1e-9 { total / base } else { 1.0 };
    let bound_lower = bound.to_ascii_lowercase();

    let mut steps = Vec::new();
    steps.push(
        BreakdownStep::label(format!("Base {bound_lower}"))
            .with_value(base)
            .with_explain(format!(
                "{base:.0} from skill stats (raw per-level value before player mods)"
            )),
    );
    steps.push(
        BreakdownStep::label("Multiplier")
            .with_value(mult)
            .with_explain(format!(
                "{mult:.2}× from (1 + INC) × MORE × (1 + quality/200) — see MainSkillAverageHit for the per-step decomposition"
            )),
    );
    steps.push(
        BreakdownStep::label(format!("Hit {bound_lower}"))
            .with_value(total)
            .with_explain(format!("{base:.0} × {mult:.2} = {total:.1}")),
    );

    Some(Breakdown {
        output_key: format!("MainSkillHit{bound}"),
        total,
        steps,
    })
}

/// Issue #34 follow-up: re-derive `SecondMinimalMaximumHitTaken`.
/// PoB sorts the five per-element MaximumHitTaken values and picks
/// the second-smallest as the "next worst max hit" — what the build
/// can take if the smallest element's incoming damage is mitigated.
/// Surfacing the chain shows all five candidates plus which one
/// won.
///
/// Returns `None` when the per-element max-hits aren't yet
/// populated (no character loaded yet).
fn second_min_max_hit_taken(env: &Env) -> Option<Breakdown> {
    let entries: Vec<(&str, f64)> = ["Physical", "Fire", "Cold", "Lightning", "Chaos"]
        .iter()
        .map(|elem| {
            let v = env.output.get(&format!("{elem}MaximumHitTaken"));
            (*elem, v)
        })
        .collect();
    if entries.iter().all(|(_, v)| v.abs() < 1e-9) {
        return None;
    }
    let total = env.output.get("SecondMinimalMaximumHitTaken");
    let mut sorted = entries.clone();
    sorted.sort_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal));
    let smallest = sorted[0].0;
    let second = sorted[1].0;

    let mut steps = Vec::new();
    for (elem, value) in &entries {
        steps.push(
            BreakdownStep::label(*elem)
                .with_value(*value)
                .with_explain(format!(
                    "{value:.0} from {elem}MaximumHitTaken — see its breakdown"
                )),
        );
    }
    steps.push(
        BreakdownStep::label("Second-smallest max hit")
            .with_value(total)
            .with_explain(format!(
                "smallest = {smallest} ({:.0}); second = {second} ({:.0}) — picks the second-smallest as 'next worst max hit'",
                sorted[0].1, sorted[1].1
            )),
    );

    Some(Breakdown {
        output_key: "SecondMinimalMaximumHitTaken".to_owned(),
        total,
        steps,
    })
}

/// Issue #34 follow-up: shared helper for the five
/// `<Element>MaximumHitTaken` outputs. PoB derives all five through
/// the same shape (`perform_ehp`):
///
///   max_hit = min(<Element>EHP, pool × 10)
///
/// — pool × 10 is PoB's defence-panel cap that prevents unrealistic
/// numbers when an element has high effective resistance /
/// suppression layered. Surfacing the chain shows users whether
/// their max-hit is rate-limited (= EHP) or cap-limited
/// (= pool × 10).
///
/// `elem` is the canonical name (`"Physical"`, `"Fire"`, etc.); the
/// EHP and final outputs are looked up by suffix.
///
/// Returns `None` when the pool is zero (no character loaded yet).
fn maximum_hit_taken(env: &Env, elem: &str) -> Option<Breakdown> {
    let life = env.output.get("Life");
    let es = env.output.get("EnergyShield");
    let ward = env.output.get("Ward");
    let pool = life + es + ward;
    if pool.abs() < 1e-9 {
        return None;
    }
    let ehp = env.output.get(&format!("{elem}EHP"));
    let cap = pool * 10.0;
    let total = env.output.get(&format!("{elem}MaximumHitTaken"));
    let capped = (total - cap).abs() < 1e-9 && ehp > cap;
    let elem_lower = elem.to_ascii_lowercase();

    let mut steps = Vec::new();
    steps.push(
        BreakdownStep::label("Effective HP")
            .with_value(ehp)
            .with_explain(format!(
                "{ehp:.0} from {elem}EHP — see its breakdown for the pool × resist / suppress chain"
            )),
    );
    steps.push(
        BreakdownStep::label("Cap")
            .with_value(cap)
            .with_explain(format!(
                "{pool:.0} pool × 10 = {cap:.0} (PoB defence-panel cap)"
            )),
    );
    let final_explain = if capped {
        format!("min({ehp:.0}, {cap:.0}) = {total:.0} (capped at pool × 10)")
    } else {
        format!("min({ehp:.0}, {cap:.0}) = {total:.0}")
    };
    steps.push(
        BreakdownStep::label(format!("{elem} maximum hit taken"))
            .with_value(total)
            .with_explain(final_explain),
    );
    let _ = elem_lower;

    Some(Breakdown {
        output_key: format!("{elem}MaximumHitTaken"),
        total,
        steps,
    })
}

/// Issue #34 follow-up: shared helper for default-plus-BASE-mods
/// chain outputs. Covers the four max-resist outputs and
/// `BlockChanceMax` — both shapes follow:
///
///   max = default + Σ BASE("<output_key>")
///
/// `default_explain` and `mods_hint` carry the per-cap PoE-context
/// strings (resist mods come from Loreweave / Purity / asc, block
/// max from Glancing Blows / Bone Offering / asc).
///
/// Returns `None` when the max is zero (no character loaded yet).
fn default_plus_base_max(
    env: &Env,
    output_key: &str,
    final_label: &str,
    default: f64,
    default_explain: &str,
    mods_hint: &str,
) -> Option<Breakdown> {
    let total = env.output.get(output_key);
    if total.abs() < 1e-9 {
        return None;
    }
    let cfg = QueryCfg::default();
    let base_sum = env.mod_db.sum(ModType::Base, &cfg, &env.state, output_key);

    let mut steps = Vec::new();
    steps.push(
        BreakdownStep::label("Default")
            .with_value(default)
            .with_explain(default_explain.to_owned()),
    );

    if base_sum.abs() > 1e-9 {
        let base_mods: Vec<ModSource> = env
            .mod_db
            .iter_named(output_key)
            .filter(|m| m.kind == ModType::Base)
            .map(ModSource::from_mod)
            .collect();
        steps.push(
            BreakdownStep::label("BASE mods")
                .with_value(base_sum)
                .with_explain(format!(
                    "+{base_sum:.0}% from {output_key} BASE mods ({mods_hint})"
                ))
                .with_sources(base_mods),
        );
    }

    steps.push(
        BreakdownStep::label(final_label)
            .with_value(total)
            .with_explain(format!("{default:.0} + {base_sum:+.0} = {total:.0}%")),
    );

    Some(Breakdown {
        output_key: output_key.to_owned(),
        total,
        steps,
    })
}

/// Thin wrapper: the four max-resist outputs all share the
/// `default_plus_base_max` chain with a 75% default and the resist
/// mod-source hint.
fn resist_max(env: &Env, output_key: &str, final_label: &str) -> Option<Breakdown> {
    default_plus_base_max(
        env,
        output_key,
        final_label,
        75.0,
        "75% default max resist (PoE constant)",
        "Loreweave / aura / ascendancy",
    )
}

/// Issue #34 follow-up: re-derive `AccuracyHitChance`. PoB sets it
/// to the same value as `MainSkillHitChance` so character-level code
/// can read the active skill's hit chance under attack-skill
/// terminology (`perform.rs:3735`/`:3743`). The breakdown calls out
/// the alias and links back to MainSkillHitChance so users don't have
/// to wonder why the two outputs match.
///
/// Returns `None` when AccuracyHitChance is zero (no skill bound).
fn accuracy_hit_chance(env: &Env) -> Option<Breakdown> {
    let total = env.output.get("AccuracyHitChance");
    if total.abs() < 1e-9 {
        return None;
    }
    let main_skill = env.output.get("MainSkillHitChance");

    let mut steps = Vec::new();
    steps.push(
        BreakdownStep::label("Main skill hit chance")
            .with_value(main_skill)
            .with_explain(format!(
                "{main_skill:.0}% from MainSkillHitChance — see its breakdown for the Accuracy / EnemyEvasion formula"
            )),
    );
    steps.push(
        BreakdownStep::label("Accuracy hit chance")
            .with_value(total)
            .with_explain(format!(
                "{total:.0}% — alias of MainSkillHitChance exposed under attack-skill terminology"
            )),
    );

    Some(Breakdown {
        output_key: "AccuracyHitChance".to_owned(),
        total,
        steps,
    })
}

/// Issue #34 follow-up: re-derive `ManaRegenRecovery`. PoB sets it
/// to the same value as `ManaRegen` so flask-recovery code can read
/// the regen rate under that name (`perform_basic_stats:1277`). The
/// breakdown calls out the alias and links back to ManaRegen so
/// users don't have to wonder why the two outputs match.
///
/// Returns `None` when ManaRegenRecovery is zero.
fn mana_regen_recovery(env: &Env) -> Option<Breakdown> {
    let total = env.output.get("ManaRegenRecovery");
    if total.abs() < 1e-9 {
        return None;
    }
    let mana_regen = env.output.get("ManaRegen");

    let mut steps = Vec::new();
    steps.push(
        BreakdownStep::label("Mana regen")
            .with_value(mana_regen)
            .with_explain(format!(
                "{mana_regen:.1}/sec from ManaRegen — see its breakdown for the baseline + flat + inc chain"
            )),
    );
    steps.push(
        BreakdownStep::label("Mana regen recovery")
            .with_value(total)
            .with_explain(format!(
                "{total:.1}/sec — alias of ManaRegen exposed for flask-recovery code"
            )),
    );

    Some(Breakdown {
        output_key: "ManaRegenRecovery".to_owned(),
        total,
        steps,
    })
}

/// Issue #34 follow-up: shared helper for ailment-chance outputs
/// that compose on-hit + on-crit (=100%) per PoB's `combine`:
///
///   chance = on_hit × (1 - crit) + 100 × crit
///
/// Crits always inflict the ailment (PoE rule for Ignite / Shock /
/// Freeze), so the on-crit term is always 100%. Surfaces both
/// component chances + crit weighting so users can see how the
/// final percentage composes.
///
/// `ailment` is the lowercase noun (`"ignite"`, `"shock"`, `"freeze"`)
/// used in the explain strings; `output_key` and `on_hit_key` /
/// `final_label` select the fields and labels.
fn on_hit_plus_crit_chance(
    env: &Env,
    output_key: &str,
    on_hit_key: &str,
    final_label: &str,
    ailment: &str,
) -> Option<Breakdown> {
    let total = env.output.get(output_key);
    if total.abs() < 1e-9 {
        return None;
    }
    let on_hit = env.output.get(on_hit_key);
    let on_crit = 100.0;
    let crit_pct = env.output.get("MainSkillCritChance");
    let crit = crit_pct / 100.0;

    let mut steps = Vec::new();
    steps.push(
        BreakdownStep::label("On-hit chance")
            .with_value(on_hit)
            .with_explain(format!(
                "{on_hit:.0}% from {on_hit_key} (gem / curse / passive)"
            )),
    );
    steps.push(
        BreakdownStep::label("On-crit chance")
            .with_value(on_crit)
            .with_explain(format!("100% — crits always {ailment} (PoE rule)")),
    );
    steps.push(
        BreakdownStep::label("Crit chance")
            .with_value(crit_pct)
            .with_explain(format!("{crit_pct:.1}% from MainSkillCritChance")),
    );
    steps.push(
        BreakdownStep::label(final_label)
            .with_value(total)
            .with_explain(format!(
                "{on_hit:.0}% × (1 - {crit:.2}) + 100% × {crit:.2} = {total:.1}%"
            )),
    );

    Some(Breakdown {
        output_key: output_key.to_owned(),
        total,
        steps,
    })
}

/// Issue #34 follow-up: re-derive `IgniteChance`. Thin wrapper
/// around the shared `on_hit_plus_crit_chance` helper.
fn ignite_chance(env: &Env) -> Option<Breakdown> {
    on_hit_plus_crit_chance(
        env,
        "IgniteChance",
        "IgniteChanceOnHit",
        "Ignite chance",
        "ignite",
    )
}

/// Issue #34 follow-up: re-derive `BleedChance`. PoB stores it as
/// a 0-100 percentage derived from `clamp(0, 100, Σ BASE("BleedChance"))`.
/// Surfacing the chain shows the contributing source mods so users
/// can see what's driving their bleed chance.
///
/// Returns `None` when the chance is zero (no mods).
fn bleed_chance(env: &Env) -> Option<Breakdown> {
    let total = env.output.get("BleedChance");
    if total.abs() < 1e-9 {
        return None;
    }
    let cfg = QueryCfg::default();
    let base_sum = env
        .mod_db
        .sum(ModType::Base, &cfg, &env.state, "BleedChance");
    let capped = base_sum > 100.0 + 1e-9;

    let mut steps = Vec::new();
    let base_mods: Vec<ModSource> = env
        .mod_db
        .iter_named("BleedChance")
        .filter(|m| m.kind == ModType::Base)
        .map(ModSource::from_mod)
        .collect();
    steps.push(
        BreakdownStep::label("BASE mods")
            .with_value(base_sum)
            .with_explain(format!(
                "+{base_sum:.0}% from BleedChance BASE mods (gem / passive / gear)"
            ))
            .with_sources(base_mods),
    );

    let final_explain = if capped {
        format!("clamp(0, 100, {base_sum:.0}%) = {total:.0}% (capped at 100%)")
    } else {
        format!("clamp(0, 100, {base_sum:.0}%) = {total:.0}%")
    };
    steps.push(
        BreakdownStep::label("Bleed chance")
            .with_value(total)
            .with_explain(final_explain),
    );

    Some(Breakdown {
        output_key: "BleedChance".to_owned(),
        total,
        steps,
    })
}

/// Issue #34 follow-up: re-derive `PoisonStackLimit`. PoB takes
/// `max(50 default, Σ BASE(PoisonStackLimit))` so uniques like
/// Volkuur's that explicitly raise the cap stack on top of the
/// default 50, while builds with no such mod land at exactly 50.
/// Surfacing the chain shows the default + mod sources.
///
/// Returns `Option<Breakdown>` rather than `Option<Option<...>>`
/// because the limit is always defined (default 50) once a build
/// loads — but we still skip when the output is zero (no skill
/// loaded).
fn poison_stack_limit(env: &Env) -> Option<Breakdown> {
    let total = env.output.get("PoisonStackLimit");
    if total.abs() < 1e-9 {
        return None;
    }
    let cfg = QueryCfg::default();
    let base_sum = env
        .mod_db
        .sum(ModType::Base, &cfg, &env.state, "PoisonStackLimit");
    let default = 50.0;

    let mut steps = Vec::new();
    steps.push(
        BreakdownStep::label("Default")
            .with_value(default)
            .with_explain("50 default cap (PoE constant)".to_owned()),
    );

    if base_sum.abs() > 1e-9 {
        let base_mods: Vec<ModSource> = env
            .mod_db
            .iter_named("PoisonStackLimit")
            .filter(|m| m.kind == ModType::Base)
            .map(ModSource::from_mod)
            .collect();
        steps.push(
            BreakdownStep::label("BASE mods")
                .with_value(base_sum)
                .with_explain(format!(
                    "+{base_sum:.0} from PoisonStackLimit BASE mods (uniques like Volkuur's, etc.)"
                ))
                .with_sources(base_mods),
        );
    }

    steps.push(
        BreakdownStep::label("Poison stack limit")
            .with_value(total)
            .with_explain(format!("max({default:.0}, {base_sum:+.0}) = {total:.0}")),
    );

    Some(Breakdown {
        output_key: "PoisonStackLimit".to_owned(),
        total,
        steps,
    })
}

/// Issue #34 follow-up: re-derive `PoisonStacks`. PoB computes
/// steady-state stack count as
/// `min(MainSkillSpeed × PoisonDuration × poison_chance, PoisonStackLimit)`.
/// The chance isn't directly stored, so we back-derive it from the
/// stack count when the cap isn't active. Surfacing the
/// composition lets users see whether they're cap-limited (cap == result)
/// or rate-limited (chance × speed × duration == result).
///
/// Returns `None` when the build doesn't inflict poison.
fn poison_stacks(env: &Env) -> Option<Breakdown> {
    let stacks = env.output.get("PoisonStacks");
    if stacks.abs() < 1e-9 {
        return None;
    }
    let speed = env.output.get("MainSkillSpeed");
    let duration = env.output.get("PoisonDuration");
    let limit = env.output.get("PoisonStackLimit");
    let nominal = speed * duration;
    let chance = if nominal > 1e-9 {
        stacks / nominal
    } else {
        0.0
    };
    // Cap detection: PoB caps `stacks = min(speed × duration × chance, limit)`.
    // When the result equals the limit the cap saturated the row;
    // we treat that as cap-limited and label the explain accordingly.
    let capped = limit > 0.0 && (stacks - limit).abs() < 1e-9;

    let mut steps = Vec::new();
    steps.push(
        BreakdownStep::label("Skill speed")
            .with_value(speed)
            .with_explain(format!("{speed:.2} cps from MainSkillSpeed")),
    );
    steps.push(
        BreakdownStep::label("Poison duration")
            .with_value(duration)
            .with_explain(format!(
                "{duration:.1}s — see PoisonDuration for the base × inc chain"
            )),
    );
    steps.push(
        BreakdownStep::label("Effective chance")
            .with_value(chance)
            .with_explain(format!(
                "{:.0}% (back-derived from stacks ÷ speed × duration)",
                chance * 100.0
            )),
    );
    steps.push(
        BreakdownStep::label("Stack limit")
            .with_value(limit)
            .with_explain(format!(
                "{limit:.0} from PoisonStackLimit (default 50, raised by uniques like Volkuur's)"
            )),
    );
    let final_explain = if capped {
        format!("min({nominal:.1}, {limit:.0}) = {stacks:.1} (capped at stack limit)")
    } else {
        format!(
            "{speed:.2} × {duration:.1} × {:.0}% = {stacks:.1}",
            chance * 100.0
        )
    };
    steps.push(
        BreakdownStep::label("Poison stacks")
            .with_value(stacks)
            .with_explain(final_explain),
    );

    Some(Breakdown {
        output_key: "PoisonStacks".to_owned(),
        total: stacks,
        steps,
    })
}

/// Issue #34 follow-up: shared helper for ailment-duration outputs
/// (Ignite / Poison / Bleed). All three follow the same shape in
/// `perform_skill_dps`:
///
///   total = base × (1 + Σ INC(<key>Duration) / 100)
///
/// The base differs per ailment (Ignite 4s, Poison 2s, Bleed 5s) but
/// the inc chain is identical. Surfacing the chain makes the inc-mod
/// sources visible alongside the seconds value.
///
/// Returns `None` when the duration is zero (not yet computed for
/// the current build / skill).
fn ailment_duration(
    env: &Env,
    output_key: &str,
    base: f64,
    base_explain: &str,
    final_label: &str,
) -> Option<Breakdown> {
    let total = env.output.get(output_key);
    if total.abs() < 1e-9 {
        return None;
    }
    let cfg = QueryCfg::default();
    let inc_total = env.mod_db.sum(ModType::Inc, &cfg, &env.state, output_key);

    let mut steps = Vec::new();
    steps.push(
        BreakdownStep::label("Base")
            .with_value(base)
            .with_explain(base_explain.to_owned()),
    );

    if inc_total.abs() > 1e-9 {
        let inc_mods: Vec<ModSource> = env
            .mod_db
            .iter_named(output_key)
            .filter(|m| m.kind == ModType::Inc)
            .map(ModSource::from_mod)
            .collect();
        steps.push(
            BreakdownStep::label("Increased")
                .with_value(1.0 + inc_total / 100.0)
                .with_explain(format!("{inc_total:+.0}% sum"))
                .with_sources(inc_mods),
        );
    }

    steps.push(
        BreakdownStep::label(final_label)
            .with_value(total)
            .with_explain(format!("{base:.0} × (1 + {inc_total:.0}%) = {total:.2}s")),
    );

    Some(Breakdown {
        output_key: output_key.to_owned(),
        total,
        steps,
    })
}

/// Issue #34 follow-up: re-derive `IgniteDuration`. Thin wrapper
/// around the shared `ailment_duration` helper with the 4s base.
fn ignite_duration(env: &Env) -> Option<Breakdown> {
    ailment_duration(
        env,
        "IgniteDuration",
        4.0,
        "4s base ignite duration (PoE constant)",
        "Ignite duration",
    )
}

/// Issue #34 follow-up: re-derive `MainSkillShockMult`. PoB applies
/// shock as `EnemyDamageTaken INC` weighted by an effective shock
/// chance:
///
///   chance = CurseShockChanceOnHit × (1 - crit) + 100 × crit
///   mult   = 1 + chance / 100
///
/// (PoB only enables the path when CurseShockChanceOnHit > 0; pure
/// crit-driven ShockChance from spells doesn't propagate.)
///
/// When the gate is off the multiplier collapses to 1.0× and the
/// breakdown shows just the final "no shock active" line. When the
/// gate is on it walks Curse → Crit → Effective chance → final.
fn main_skill_shock_mult(env: &Env) -> Breakdown {
    let mult = env.output.get("MainSkillShockMult");
    let curse = env.output.get("CurseShockChanceOnHit");

    let mut steps = Vec::new();
    if curse > 0.0 {
        let crit_pct = env.output.get("MainSkillCritChance");
        let crit = crit_pct / 100.0;
        let chance = (curse * (1.0 - crit) + 100.0 * crit).clamp(0.0, 100.0);
        steps.push(
            BreakdownStep::label("Curse-on-hit chance")
                .with_value(curse)
                .with_explain(format!(
                    "{curse:.0}% from CurseShockChanceOnHit (curse / brand / item)"
                )),
        );
        steps.push(
            BreakdownStep::label("Crit chance")
                .with_value(crit_pct)
                .with_explain(format!(
                    "{crit_pct:.1}% — crits always shock for 100% effective chance"
                )),
        );
        steps.push(
            BreakdownStep::label("Effective shock chance")
                .with_value(chance)
                .with_explain(format!(
                    "{curse:.0} × (1 - {crit:.2}) + 100 × {crit:.2} = {chance:.1}%"
                )),
        );
        steps.push(
            BreakdownStep::label("Shock multiplier")
                .with_value(mult)
                .with_explain(format!("1 + {chance:.1}% = {mult:.2}×")),
        );
    } else {
        steps.push(
            BreakdownStep::label("Shock multiplier")
                .with_value(mult)
                .with_explain(
                    "1.0× — no curse / brand / item source of shock-on-hit, so PoB suppresses the dynamic-effect mult".to_owned(),
                ),
        );
    }

    Breakdown {
        output_key: "MainSkillShockMult".to_owned(),
        total: mult,
        steps,
    }
}

/// Issue #34 follow-up: re-derive `ProjectileMultiplier`. PoB caps
/// the user's "projectiles hitting target" Config pick into
/// `[1, ProjectileCount]` and uses the result as a per-hit damage
/// multiplier (focal-point Tornado Shot, point-blank Barrage, etc.).
/// The Calcs tab surfaced the multiplier as a single number; users
/// couldn't see whether the cap was active or how the requested-vs-
/// actual hits compared.
///
/// The breakdown back-derives the requested count from the final
/// multiplier (since the multiplier IS the capped requested count)
/// and surfaces both the cap and a `capped` hint when the multiplier
/// equals the projectile count.
///
/// Returns `None` when the skill isn't projectile-based.
fn projectile_multiplier(env: &Env) -> Option<Breakdown> {
    let count = env.output.get("ProjectileCount");
    if count.abs() < 1e-9 {
        return None;
    }
    let multiplier = env.output.get("ProjectileMultiplier");
    let capped = (multiplier - count).abs() < 1e-9;
    let final_explain = if capped {
        format!("min({multiplier:.0}, {count:.0}) = {multiplier:.0} (capped at projectile count)")
    } else {
        format!("min({multiplier:.0}, {count:.0}) = {multiplier:.0}")
    };

    let mut steps = Vec::new();
    steps.push(
        BreakdownStep::label("Hits requested")
            .with_value(multiplier)
            .with_explain(format!(
                "{multiplier:.0} from Config: \"projectiles hitting target\""
            )),
    );
    steps.push(
        BreakdownStep::label("Cap")
            .with_value(count)
            .with_explain(format!(
                "{count:.0} from ProjectileCount — see its breakdown for the primary + additional split"
            )),
    );
    steps.push(
        BreakdownStep::label("Projectile multiplier")
            .with_value(multiplier)
            .with_explain(final_explain),
    );

    Some(Breakdown {
        output_key: "ProjectileMultiplier".to_owned(),
        total: multiplier,
        steps,
    })
}

/// Issue #34 follow-up: re-derive `ProjectileCount`. PoB's
/// `perform_skill_dps` computes the total as
/// `1 (primary) + Σ number_of_additional_projectiles`. Surfacing
/// the chain shows the primary + additional split so users can see
/// what their LMP / GMP / etc. mods contribute.
///
/// Returns `None` when the skill isn't projectile-based (the
/// output is missing or zero).
fn projectile_count(env: &Env) -> Option<Breakdown> {
    let total = env.output.get("ProjectileCount");
    if total.abs() < 1e-9 {
        return None;
    }
    let additional = (total - 1.0).max(0.0);

    let mut steps = Vec::new();
    steps.push(
        BreakdownStep::label("Primary")
            .with_value(1.0)
            .with_explain("1 primary projectile (PoE constant)".to_owned()),
    );
    if additional.abs() > 1e-9 {
        steps.push(
            BreakdownStep::label("Additional")
                .with_value(additional)
                .with_explain(format!(
                    "+{additional:.0} from skill / tree / gear additional-projectile mods"
                )),
        );
    }
    steps.push(
        BreakdownStep::label("Projectile count")
            .with_value(total)
            .with_explain(format!("1 + {additional:.0} = {total:.0}")),
    );

    Some(Breakdown {
        output_key: "ProjectileCount".to_owned(),
        total,
        steps,
    })
}

/// Issue #34 follow-up: re-derive `AreaOfEffectRadiusMetres`. PoB
/// exposes the AoE radius in metres alongside the engine units;
/// conversion is just `engine_units / 10`. Surfacing the chain lets
/// users see the engine-units value next to the metres value
/// without context-switching back to AreaOfEffectRadius.
///
/// Returns `None` when the engine-unit radius is zero.
fn area_of_effect_radius_metres(env: &Env) -> Option<Breakdown> {
    let units = env.output.get("AreaOfEffectRadius");
    if units.abs() < 1e-9 {
        return None;
    }
    let metres = env.output.get("AreaOfEffectRadiusMetres");

    let mut steps = Vec::new();
    steps.push(
        BreakdownStep::label("Radius (engine units)")
            .with_value(units)
            .with_explain(format!(
                "{units:.0} from AreaOfEffectRadius — see its breakdown for the base × sqrt(mod) chain"
            )),
    );
    steps.push(
        BreakdownStep::label("Area of effect radius (metres)")
            .with_value(metres)
            .with_explain(format!("{units:.0} / 10 = {metres:.2} m")),
    );

    Some(Breakdown {
        output_key: "AreaOfEffectRadiusMetres".to_owned(),
        total: metres,
        steps,
    })
}

/// Issue #34 follow-up: re-derive `AreaOfEffectRadius`. Mirrors
/// `perform_skill_dps`'s AoE block:
///
///   radius = floor(base × floor(100 × sqrt(area_mod)) / 100)
///
/// — the inner floor mirrors PoB's two-decimal rounding before the
/// outer integer floor. The sqrt is what makes %AoE non-linear vs
/// circle radius (a +50% area mod is only a +22% radius increase).
///
/// Returns `None` when the active skill has no base AoE radius
/// (non-AoE skills, or builds with no skill selected) so the panel
/// falls back to the generic mods view.
fn area_of_effect_radius(env: &Env) -> Option<Breakdown> {
    let base = env.output.get("AoERadius");
    if base.abs() < 1e-9 {
        return None;
    }
    let area_mod = env.output.get("AreaOfEffectMod");
    let scale = area_mod.max(0.0).sqrt();
    let total = env.output.get("AreaOfEffectRadius");

    let mut steps = Vec::new();
    steps.push(
        BreakdownStep::label("Base radius")
            .with_value(base)
            .with_explain(format!(
                "{base:.0} from skill base + AreaOfEffect BASE mods"
            )),
    );
    steps.push(
        BreakdownStep::label("Area mod")
            .with_value(area_mod)
            .with_explain(format!(
                "{area_mod:.2}× — see AreaOfEffectMod for the (1+inc) × more derivation"
            )),
    );
    steps.push(
        BreakdownStep::label("Radius scaling")
            .with_value(scale)
            .with_explain(format!(
                "sqrt({area_mod:.2}) = {scale:.3} — radius scales by sqrt of area"
            )),
    );
    steps.push(
        BreakdownStep::label("Area of effect radius")
            .with_value(total)
            .with_explain(format!(
                "floor({base:.0} × floor(100 × {scale:.3}) / 100) = {total:.0}"
            )),
    );

    Some(Breakdown {
        output_key: "AreaOfEffectRadius".to_owned(),
        total,
        steps,
    })
}

/// Issue #34 follow-up: re-derive `EnergyShieldRecharge`. Mirrors
/// `perform_basic_stats`:
///
///   recharge = EnergyShield × 0.33 × (1 + Σ INC(EnergyShieldRecharge) / 100)
///
/// 33% per second of the player's ES pool is the PoE constant once
/// the recharge delay (default 2s, modified by Faster Start of Energy
/// Shield Recharge mods on a separate stat) elapses. Surfaced steps:
/// baseline (33% × ES), INC scaling (only when non-zero), and the
/// final per-second rate.
///
/// Returns `None` when the player has no ES pool — the panel falls
/// back to the generic contributing-modifiers view rather than
/// rendering a `0/sec` row with no context.
fn energy_shield_recharge(env: &Env) -> Option<Breakdown> {
    let es = env.output.get("EnergyShield");
    if es.abs() < 1e-9 {
        return None;
    }
    let cfg = QueryCfg::default();
    let inc_total = env
        .mod_db
        .sum(ModType::Inc, &cfg, &env.state, "EnergyShieldRecharge");
    let baseline = es * 0.33;
    let total = env.output.get("EnergyShieldRecharge");

    let mut steps = Vec::new();
    steps.push(
        BreakdownStep::label("Baseline")
            .with_value(baseline)
            .with_explain(format!(
                "33% × {es:.0} ES = {baseline:.1}/sec (PoE constant)"
            )),
    );

    if inc_total.abs() > 1e-9 {
        let inc_mods: Vec<ModSource> = env
            .mod_db
            .iter_named("EnergyShieldRecharge")
            .filter(|m| m.kind == ModType::Inc)
            .map(ModSource::from_mod)
            .collect();
        steps.push(
            BreakdownStep::label("Increased")
                .with_value(1.0 + inc_total / 100.0)
                .with_explain(format!("{inc_total:+.0}% sum"))
                .with_sources(inc_mods),
        );
    }

    steps.push(
        BreakdownStep::label("Energy shield recharge")
            .with_value(total)
            .with_explain(format!(
                "{baseline:.1} × (1 + {inc_total:.0}%) = {total:.1}/sec"
            )),
    );

    Some(Breakdown {
        output_key: "EnergyShieldRecharge".to_owned(),
        total,
        steps,
    })
}

fn source_label(s: Option<&Source>) -> String {
    match s {
        Some(Source::Tree) => "tree".into(),
        Some(Source::Passive(id)) => format!("passive: #{id}"),
        Some(Source::Ascendancy(s)) => format!("ascendancy: {s}"),
        Some(Source::Item(slot)) => format!("item slot {slot}"),
        Some(Source::Skill(id)) => format!("skill: {id}"),
        Some(Source::Buff(id)) => format!("buff: {id}"),
        Some(Source::Config(id)) => format!("config: {id}"),
        Some(Source::Other(s)) => s.clone(),
        None => "(unknown)".into(),
    }
}

/// Issue #34 (#220 follow-up): re-derive a pool stat (Life / Mana)
/// that draws from a primary attribute on top of the standard
/// `BASE → INC → MORE` chain. Mirrors `perform_basic_stats`:
///
///   pool_base = Σ BASE(<key>) + Σ BASE(<other-attribute keys>) + attribute / divisor
///   pool      = pool_base × (1 + INC/100) × MORE
///
/// `attribute` is the player attribute that contributes to the pool —
/// `Strength` for Life (1 Str → +0.5 Life) and `Intelligence` for Mana
/// (1 Int → +0.5 Mana). `divisor` is `2.0` for both PoE pools today;
/// kept as a parameter so future pools that scale at different rates
/// (Path of Exile 2 changes the curve) can plug in cleanly without
/// duplicating the BASE / INC / MORE wiring.
fn pool_with_attribute(env: &Env, key: &str, attribute: &str, divisor: f64) -> Breakdown {
    let cfg = QueryCfg::default();
    let base_sum = env.mod_db.sum(ModType::Base, &cfg, &env.state, key);
    let attribute_value = env.output.get(attribute);
    let attribute_contrib = attribute_value / divisor;
    let pool_base = base_sum + attribute_contrib;
    let inc_total = env.mod_db.sum(ModType::Inc, &cfg, &env.state, key);
    let more_total = env.mod_db.more(&cfg, &env.state, key);
    let total = env.output.get(key);

    let mut steps = Vec::new();
    // Base: collapse "+X to maximum <pool>" mods (item rolls, tree
    // notables, ascendancy small grants) into one row with each
    // contributor surfaced in the source list. The attribute
    // contribution is shown separately so the user can see how their
    // Str / Int investment is paying off.
    let base_mods: Vec<ModSource> = env
        .mod_db
        .iter_named(key)
        .filter(|m| m.kind == ModType::Base)
        .map(ModSource::from_mod)
        .collect();
    let base_explain = format!(
        "+{base_sum:.0} from BASE mods + {attribute_value:.0} {attribute} / {divisor:.0} = {attribute_contrib:.1}",
    );
    steps.push(
        BreakdownStep::label("Base")
            .with_value(pool_base)
            .with_explain(base_explain)
            .with_sources(base_mods),
    );

    let mut inc_mods: Vec<ModSource> = env
        .mod_db
        .iter_named(key)
        .filter(|m| m.kind == ModType::Inc)
        .map(ModSource::from_mod)
        .collect();
    if !inc_mods.is_empty() || inc_total != 0.0 {
        inc_mods.sort_by(|a, b| {
            b.value
                .unwrap_or(0.0)
                .abs()
                .partial_cmp(&a.value.unwrap_or(0.0).abs())
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        steps.push(
            BreakdownStep::label("Increased")
                .with_value(1.0 + inc_total / 100.0)
                .with_explain(format!("{inc_total:+.0}% sum"))
                .with_sources(inc_mods),
        );
    }

    let mut more_mods: Vec<ModSource> = env
        .mod_db
        .iter_named(key)
        .filter(|m| m.kind == ModType::More)
        .map(ModSource::from_mod)
        .collect();
    if !more_mods.is_empty() || (more_total - 1.0).abs() > 1e-9 {
        more_mods.sort_by(|a, b| {
            b.value
                .unwrap_or(0.0)
                .abs()
                .partial_cmp(&a.value.unwrap_or(0.0).abs())
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        steps.push(
            BreakdownStep::label("More")
                .with_value(more_total)
                .with_explain("multiplicative".to_owned())
                .with_sources(more_mods),
        );
    }

    steps.push(
        BreakdownStep::label(key)
            .with_value(total)
            .with_explain(format!(
                "{pool_base:.0} × (1 + {inc_total:.0}%) × {more_total:.3} = {total:.0}"
            )),
    );

    Breakdown {
        output_key: key.to_owned(),
        total,
        steps,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::env::Env;
    use crate::modifier::{Mod, Source};

    fn env_with_output() -> Env {
        let mut env = Env::default();
        // Set a representative skill output: Arc-shape spell average hit.
        env.output.set("MainSkillBaseMin", 100.0);
        env.output.set("MainSkillBaseMax", 200.0);
        env.output.set("MainSkillHitMin", 220.0);
        env.output.set("MainSkillHitMax", 440.0);
        env.output.set("MainSkillAverageHit", 330.0);
        env.output.set("MainSkillCritChance", 6.0);
        env.output.set("CritMultiplier", 1.5);
        env.output.set("CritEffect", 1.03);
        env.output.set("MainSkillSpeed", 5.0);
        env.output.set("MainSkillDPS", 1650.0);
        env.output.set("CastSpeedMult", 1.30);
        env.output.set("AttackSpeedMult", 1.0);
        env.output.set("MovementSpeedMod", 1.20);
        env.output.set("AreaOfEffectMod", 1.0);
        // Issue #34 follow-up: representative AoE base/final radius
        // so the COVERED_KEYS guard exercises AreaOfEffectRadius.
        // Base 22 (Arc default) × sqrt(1.0) = 22.
        env.output.set("AoERadius", 22.0);
        env.output.set("AreaOfEffectRadius", 22.0);
        env.output.set("AreaOfEffectRadiusMetres", 2.2);
        // Issue #34 follow-up: representative projectile count so the
        // COVERED_KEYS guard exercises the breakdown. 3 = 1 primary +
        // 2 additional (LMP-shape).
        env.output.set("ProjectileCount", 3.0);
        // Issue #34 follow-up: spell-shape baseline — 1 projectile
        // requested by Config, capped at 3. Exercises the
        // ProjectileMultiplier breakdown via the COVERED_KEYS guard.
        env.output.set("ProjectileMultiplier", 1.0);
        // Issue #34 follow-up: representative ailment durations so
        // the COVERED_KEYS guard exercises the Ignite / Poison /
        // Bleed duration breakdowns. PoE constants: ignite 4s,
        // poison 2s, bleed 5s — all at the no-inc baseline.
        env.output.set("IgniteDuration", 4.0);
        env.output.set("PoisonDuration", 2.0);
        env.output.set("BleedDuration", 5.0);
        env.output.set("CritChance", 6.0);
        env.output.set("FullDPS", 2000.0);
        env.output.set("BleedDPS", 0.0);
        env.output.set("PoisonDPS", 350.0);
        // Issue #34 follow-up: PoisonDPS = per_stack × stacks. With
        // PoisonDPS = 350 and 10 steady-state stacks → per_stack = 35.
        // Real Arc-style spell builds at 5 cps × 2s base duration ×
        // 100% chance land around 10–15 stacks against PoB's standard
        // boss preset.
        env.output.set("PoisonStacks", 10.0);
        // Issue #34 follow-up: PoisonStackLimit default (50) so the
        // PoisonStacks breakdown has a cap to surface.
        env.output.set("PoisonStackLimit", 50.0);
        env.output.set("IgniteDPS", 0.0);
        // Issue #34 follow-up: Bleed and Ignite DPS = per_application
        // × chance. The fixture's spell build has neither active by
        // default; we re-pin both in the relevant tests with non-zero
        // values + chance outputs so the back-derived per-application
        // step has something to walk.
        env.output.set("BleedChance", 0.0);
        env.output.set("IgniteChance", 0.0);
        // Issue #34 follow-up: With{Bleed,Poison,Ignite,Impale}DPS
        // breakdowns. PoisonDPS = 350 → WithPoisonDPS = 1650 (hit) +
        // 350 = 2000. The other ailments are zero so the With… values
        // collapse to MainSkillDPS. ImpaleDPS = 0 keeps the test fixture
        // matching a pure-spell build (no impale layer).
        env.output.set("ImpaleDPS", 0.0);
        env.output.set("WithBleedDPS", 1650.0);
        env.output.set("WithPoisonDPS", 2000.0);
        env.output.set("WithIgniteDPS", 1650.0);
        env.output.set("WithImpaleDPS", 1650.0);
        env.output.set("MainSkillAverageHitAfterResist", 280.0);
        env.output.set("MainSkillAverageHitAfterShock", 280.0);
        // Issue #34 follow-up: shock multiplier defaults to 1.0 (no
        // shock active). Tests that exercise shock override this.
        env.output.set("MainSkillShockMult", 1.0);
        env.output.set("MainSkillAverageHitAfterAccuracy", 280.0);
        env.output.set("MainSkillEnemyEffectiveResist", 30.0);
        env.output.set("MainSkillHitChance", 100.0);
        env.output.set("GemQuality", 20.0);
        // Issue #34 follow-up: MainSkillManaCost breakdown reads the
        // final cost from output and back-derives the base from the
        // ManaCost Inc total. Fixture: base 16 mana, -25% Inc cost
        // (a Lifetap support / mana-efficiency build) → 12 final.
        env.output.set("MainSkillManaCost", 12.0);
        env.mod_db
            .add(Mod::inc("ManaCost", -25.0).with_source(Source::Tree));
        // Issue #34 follow-up: ManaPerSecondCost = mana_cost × cps.
        // 12 × 5 = 60 with the existing MainSkillManaCost = 12 and
        // MainSkillSpeed = 5.0 already in the fixture.
        env.output.set("ManaPerSecondCost", 60.0);
        // Issue #34 follow-up: NumberOfDamagingHits = pool / total_taken_hit.
        // With pool = 1100 and a representative total_taken_hit = 275
        // → NumberOfDamagingHits = 4. The test fixture mirrors PoB's
        // standard Pinnacle Boss preset baseline; real builds with
        // mitigation chains land in the 3–8 hits-to-die band.
        env.output.set("totalTakenHit", 275.0);
        env.output.set("NumberOfDamagingHits", 4.0);
        // Issue #34 follow-up: EHPSurvivalTime = hits_to_die × enemySkillTime.
        // PoB's `enemySkillTime` defaults to 0.7s — the standard
        // Pinnacle Boss tick rate. 4 hits × 0.7s = 2.8s of survival.
        env.output.set("enemySkillTime", 0.7);
        env.output.set("EHPSurvivalTime", 2.8);
        // Issue #34 follow-up: TotalEHP = hits_to_die × totalEnemyDamageIn.
        // 4 × 1500 = 6000. The 1500 mirrors PoB's standard Pinnacle
        // Boss preset (4 elements × 333 base + chaos at 133 ≈ 1465)
        // rounded to a clean fixture value.
        env.output.set("totalEnemyDamageIn", 1500.0);
        env.output.set("TotalEHP", 6000.0);
        // Issue #34 follow-up: Impale defaults — phys-attack builds
        // running the impale support get all four contributors set.
        // Spell builds leave them at zero; the breakdown handles both.
        // 400 stored × 10% effect × 5 stacks × 100% chance × 5 cps = 1000.
        env.output.set("ImpaleStoredHitAvg", 400.0);
        env.output.set("ImpaleEffect", 10.0);
        env.output.set("ImpaleChance", 100.0);
        env.output.set("ImpaleDPS", 1000.0);
        // Pool outputs + their attribute drivers so the Life / Mana
        // breakdowns have something to walk. The numbers track a
        // representative L90 character: 1100 Life from 50 base + 12×89
        // class-and-level + 540 from items + 80 Str / 2.
        env.output.set("Life", 1100.0);
        env.output.set("Mana", 360.0);
        // Issue #34 follow-up: representative reservation outputs so
        // the COVERED_KEYS guard exercises Life/ManaUnreserved. Witch
        // running Discipline + Clarity reserves ~145 mana out of 360
        // (≈40%) and a single life-reservation aura ~ 350 of 1100
        // (≈32%). Both pools must end up positive after reservation.
        env.output.set("LifeReserved", 350.0);
        env.output.set("LifeReservedPercent", 31.8);
        env.output.set("LifeUnreserved", 750.0);
        env.output.set("LifeUnreservedPercent", 68.2);
        env.output.set("ManaReserved", 145.0);
        env.output.set("ManaReservedPercent", 40.3);
        env.output.set("ManaUnreserved", 215.0);
        env.output.set("ManaUnreservedPercent", 59.7);
        env.output.set("Strength", 80.0);
        env.output.set("Dexterity", 50.0);
        env.output.set("Intelligence", 60.0);
        env.output.set("TotalAttr", 190.0);
        env.output.set("LowestAttribute", 50.0);
        // min(1100 Life, 360 Mana) = 360 Mana for the spell fixture.
        env.output.set("LowestOfMaximumLifeAndMaximumMana", 360.0);
        // Issue #34 follow-up: leech-rate caps. 20% of 1100 Life =
        // 220, 20% of 360 Mana = 72 — exercises MaxLifeLeechRate /
        // MaxManaLeechRate via the COVERED_KEYS guard.
        env.output.set("MaxLifeLeechRate", 220.0);
        env.output.set("MaxManaLeechRate", 72.0);
        // Issue #34 follow-up: per-instance leech caps and rates.
        // 10% of 1100 Life = 110, 10% of 360 Mana = 36, 2% of 1100
        // Life = 22, 2% of 360 Mana = 7.2.
        env.output.set("MaxLifeLeechInstance", 110.0);
        env.output.set("MaxManaLeechInstance", 36.0);
        env.output.set("LifeLeechInstanceRate", 22.0);
        env.output.set("ManaLeechInstanceRate", 7.2);
        // Issue #34 follow-up: attribute breakdowns. Add representative
        // BASE mods so `pool_basic` has something to enumerate. The
        // class-start contribution is the bulk of each attribute on a
        // typical character; `+N to <attr>` / `+N to all attributes`
        // mods stack on top.
        env.mod_db
            .add(Mod::base("Strength", 32.0).with_source(Source::Other("class start".into())));
        env.mod_db
            .add(Mod::base("Strength", 48.0).with_source(Source::Item(2)));
        env.mod_db
            .add(Mod::base("Dexterity", 32.0).with_source(Source::Other("class start".into())));
        env.mod_db
            .add(Mod::base("Dexterity", 18.0).with_source(Source::Item(4)));
        env.mod_db
            .add(Mod::base("Intelligence", 32.0).with_source(Source::Other("class start".into())));
        env.mod_db
            .add(Mod::base("Intelligence", 28.0).with_source(Source::Item(1)));
        env.mod_db
            .add(Mod::base("Life", 540.0).with_source(Source::Item(2)));
        env.mod_db
            .add(Mod::inc("Life", 30.0).with_source(Source::Tree));
        env.mod_db
            .add(Mod::base("Mana", 200.0).with_source(Source::Item(1)));
        env.mod_db
            .add(Mod::inc("Mana", 50.0).with_source(Source::Tree));
        // Resist outputs + their underlying BASE mods so the resist
        // breakdowns have something to walk. Numbers track a typical
        // L90 character: -60 level penalty + 75 from items + 60 from
        // an "all elemental" tree umbrella → 75 raw per element →
        // capped at 75. ChaosResist sits at -55 (no umbrella adder).
        env.output.set("FireResist", 75.0);
        env.output.set("FireResistMax", 75.0);
        env.output.set("FireResistTotal", 75.0);
        env.output.set("ColdResist", 75.0);
        env.output.set("ColdResistMax", 75.0);
        env.output.set("ColdResistTotal", 75.0);
        env.output.set("LightningResist", 75.0);
        env.output.set("LightningResistMax", 75.0);
        env.output.set("LightningResistTotal", 75.0);
        env.output.set("ChaosResist", -55.0);
        env.output.set("ChaosResistMax", 75.0);
        env.output.set("ChaosResistTotal", -55.0);
        env.mod_db
            .add(Mod::base("FireResist", 75.0).with_source(Source::Item(2)));
        env.mod_db
            .add(Mod::base("ColdResist", 75.0).with_source(Source::Item(2)));
        env.mod_db
            .add(Mod::base("LightningResist", 75.0).with_source(Source::Item(2)));
        env.mod_db
            .add(Mod::base("ElementalResist", 60.0).with_source(Source::Tree));
        env.mod_db
            .add(Mod::base("ChaosResist", 5.0).with_source(Source::Item(1)));
        // Defence-pool outputs. Armour and Evasion both go through
        // `pool_basic`; INC mods sourced as Strength / Dexterity show
        // up under the Increased step. Ward is rare on real builds —
        // populate it lightly so `covered_keys_is_complete` still
        // walks the dispatch arm.
        env.output.set("Armour", 4500.0);
        env.output.set("Evasion", 3200.0);
        env.output.set("Ward", 0.0);
        // Issue #34 follow-up: PhysicalDamageReduction breakdown reads
        // Armour from output. Formula: armour / (armour + 12 × baseline)
        // capped at 90%. With Armour = 4500 against PoB's 1000-pt baseline:
        // 4500 / (4500 + 12000) ≈ 27.27%.
        env.output.set("PhysicalDamageReduction", 27.27);
        // Issue #34 follow-up: PhysicalEHP breakdown reads pool (Life
        // + ES + Ward) and the damage-taken multiplier components.
        // pool = 1100 (Life) + 0 (ES, unset) + 0 (Ward)
        // phys_taken = (1 - 0.2727) × (1 - 0.30) ≈ 0.5091
        // PhysicalEHP = 1100 / 0.5091 ≈ 2161
        env.output.set("PhysicalEHP", 2161.0);
        // Issue #34 follow-up: elemental + chaos EHP breakdowns.
        // Same shape per element: pool / [(1 - resist_after_pen) × (1 - 0.5×suppress) × (1 - spell_block)].
        // With pool=1100, suppress=0.6, spell_block=0.20, all three
        // ele resists at 75% (with 3% pen → 0.72 effective):
        //   ele_taken = 0.28 × 0.7 × 0.8 ≈ 0.1568 → EHP ≈ 7015
        // Chaos at -55% (no pen):
        //   chaos_taken = 1.55 × 0.7 × 0.8 ≈ 0.868 → EHP ≈ 1267
        env.output.set("FireEHP", 7015.0);
        env.output.set("ColdEHP", 7015.0);
        env.output.set("LightningEHP", 7015.0);
        env.output.set("ChaosEHP", 1267.0);
        // Issue #34 follow-up: aggregate EHPs.
        // Average = (2161 + 7015×3 + 1267) / 5 = 24473 / 5 = 4894.6
        // Minimum = 1267 (Chaos)
        env.output.set("AverageEHP", 4894.6);
        env.output.set("MinimumEHP", 1267.0);
        env.mod_db
            .add(Mod::base("Armour", 1500.0).with_source(Source::Item(2)));
        env.mod_db
            .add(Mod::inc("Armour", 80.0).with_source(Source::Other("Strength".into())));
        env.mod_db
            .add(Mod::base("Evasion", 1100.0).with_source(Source::Item(4)));
        env.mod_db
            .add(Mod::inc("Evasion", 50.0).with_source(Source::Other("Dexterity".into())));
        // Damage avoidance outputs + their underlying BASE mods so the
        // Block / Spell Block / Spell Suppression breakdowns have
        // something to walk. 30% block from a shield, +5% block cap
        // from a tree mastery → cap 80; spell block 20% (clamped to
        // the same 80 cap); spell suppression 60% (well under the
        // hard 100% cap).
        env.output.set("BlockChance", 30.0);
        env.output.set("BlockChanceMax", 80.0);
        env.output.set("SpellBlockChance", 20.0);
        env.output.set("SpellSuppressionChance", 60.0);
        env.mod_db
            .add(Mod::base("BlockChance", 30.0).with_source(Source::Item(2)));
        env.mod_db
            .add(Mod::base("BlockChanceMax", 5.0).with_source(Source::Tree));
        env.mod_db
            .add(Mod::base("SpellBlockChance", 20.0).with_source(Source::Item(1)));
        env.mod_db
            .add(Mod::base("SpellSuppressionChance", 60.0).with_source(Source::Tree));
        // Issue #34 follow-up: Accuracy breakdown. PoB computes
        // `Accuracy = Σ BASE(Accuracy) + 2 × (level - 1) + 2 × Dex`.
        // For an L90 character with +200 Accuracy from items and 50
        // Dex, the final reading is 200 + 178 + 100 = 478. The level
        // term is back-derived in the breakdown helper so the test
        // doesn't need to know the character level directly.
        env.output.set("Accuracy", 478.0);
        env.mod_db
            .add(Mod::base("Accuracy", 200.0).with_source(Source::Item(2)));
        // Issue #34 follow-up: HitChance breakdown reads EnemyEvasion
        // from the output (perform stores it unconditionally for the
        // breakdown helper to consume). 1500 = PoB's standard
        // map-monster baseline; combined with Accuracy=478 and the
        // hit-chance formula it yields ~92%, but tests pin the
        // values they need explicitly.
        env.output.set("EnemyEvasion", 1500.0);
        // Recovery outputs + underlying mods. Representative L90:
        // 100/sec life regen from 50 flat (item belt) + 4% LifeRegenPercent
        // tree on 1100 Life = 50 + 44 = 94/sec. Mana regen baseline at
        // 1.75% of 360 mana = 6.3/sec, with no extra mods.
        env.output.set("LifeRegen", 94.0);
        env.output.set("ManaRegen", 6.3);
        env.mod_db
            .add(Mod::base("LifeRegen", 50.0).with_source(Source::Item(8)));
        env.mod_db
            .add(Mod::base("LifeRegenPercent", 4.0).with_source(Source::Tree));
        // Tree-typed INC and MORE damage mods so the breakdown enumerates them.
        env.mod_db
            .add(Mod::inc("Damage", 50.0).with_source(Source::Tree));
        env.mod_db.add(Mod::inc("FireDamage", 30.0));
        env.mod_db
            .add(Mod::more("SpellDamage", 20.0).with_source(Source::Item(2)));
        env.mod_db
            .add(Mod::inc("CastSpeed", 10.0).with_source(Source::Tree));
        env.mod_db.add(Mod::inc("CritChance", 200.0));
        env.mod_db.add(Mod::base("CritMultiplier", 30.0));
        // Issue #34 follow-up: representative EnergyShieldRegen so
        // the COVERED_KEYS guard exercises its breakdown. A Zealot's
        // Oath ES leech ring would land ~3/sec; pin to that and a
        // tree-cluster +20% INC for the inc step.
        env.mod_db
            .add(Mod::base("EnergyShieldRegen", 3.0).with_source(Source::Item(8)));
        env.mod_db
            .add(Mod::inc("EnergyShieldRegen", 20.0).with_source(Source::Tree));
        env.output.set("EnergyShieldRegen", 3.6);
        env
    }

    #[test]
    fn covered_keys_is_complete() {
        // Every key in COVERED_KEYS must yield a non-empty breakdown.
        let env = env_with_output();
        for key in COVERED_KEYS {
            let bd =
                derive_for(&env, key).unwrap_or_else(|| panic!("derive_for({key}) returned None"));
            assert!(!bd.steps.is_empty(), "{key}: no steps");
        }
    }

    #[test]
    fn unknown_key_returns_none() {
        let env = env_with_output();
        assert!(derive_for(&env, "ThisIsNotAnOutputKey").is_none());
    }

    #[test]
    fn average_hit_breakdown_contains_base_inc_more() {
        let env = env_with_output();
        let bd = derive_for(&env, "MainSkillAverageHit").unwrap();
        let labels: Vec<&str> = bd.steps.iter().map(|s| s.label.as_str()).collect();
        assert!(labels.iter().any(|l| l.starts_with("Base")));
        assert!(labels.iter().any(|l| l.starts_with("Increased")));
        assert!(labels.iter().any(|l| l.starts_with("More")));
        assert!(labels.iter().any(|l| l.starts_with("Average hit")));
    }

    #[test]
    fn average_hit_inc_step_includes_tree_source() {
        let env = env_with_output();
        let bd = derive_for(&env, "MainSkillAverageHit").unwrap();
        let inc = bd
            .steps
            .iter()
            .find(|s| s.label == "Increased")
            .expect("INC step present");
        // The Tree-sourced INC Damage 50% mod should show up as a contributing source.
        assert!(
            inc.sources.iter().any(|s| s.source == "tree"),
            "expected tree source, got {:?}",
            inc.sources
        );
    }

    #[test]
    fn dps_breakdown_chains_speed_and_crit() {
        let env = env_with_output();
        let bd = derive_for(&env, "MainSkillDPS").unwrap();
        let labels: Vec<&str> = bd.steps.iter().map(|s| s.label.as_str()).collect();
        assert!(labels.contains(&"Average hit"));
        assert!(labels.contains(&"Crit factor"));
        assert!(labels.contains(&"Cast / attack rate"));
        assert!(labels.contains(&"DPS"));
    }

    #[test]
    fn full_dps_sums_components() {
        let env = env_with_output();
        let bd = derive_for(&env, "FullDPS").unwrap();
        // Hit DPS + Poison DPS (the only nonzero ailment) + FullDPS line.
        assert!(bd.steps.iter().any(|s| s.label == "Hit DPS"));
        assert!(bd.steps.iter().any(|s| s.label == "+ Poison DPS"));
        assert!(bd.steps.iter().any(|s| s.label == "FullDPS"));
        // Bleed DPS = 0 → step is suppressed.
        assert!(!bd.steps.iter().any(|s| s.label == "+ Bleed DPS"));
    }

    /// Issue #34 follow-up: MaxLifeLeechInstance walks Pool →
    /// per-instance cap (10%) → Final. PoE caps each individual
    /// leech instance at 10% of max pool — the slower aggregator
    /// of leech-driven sustain. Worked example: 1100 Life × 0.10 =
    /// 110.
    #[test]
    fn max_life_leech_instance_breakdown_walks_pool_and_cap() {
        let mut env = Env::default();
        env.output.set("Life", 1100.0);
        env.output.set("MaxLifeLeechInstance", 110.0);
        let bd = derive_for(&env, "MaxLifeLeechInstance").unwrap();
        let labels: Vec<&str> = bd.steps.iter().map(|s| s.label.as_str()).collect();
        assert!(labels.contains(&"Life pool"));
        assert!(labels.contains(&"Per-instance cap"));
        assert!(labels.contains(&"Max life leech instance"));

        let cap = bd
            .steps
            .iter()
            .find(|s| s.label == "Per-instance cap")
            .unwrap();
        assert!((cap.value.unwrap() - 0.10).abs() < 1e-9);
        assert!((bd.total - 110.0).abs() < 1e-9);
    }

    /// Issue #34 follow-up: LifeLeechInstanceRate walks Pool → 2%
    /// drain rate → Final. The base rate at which a single leech
    /// instance pays out: 2% of max pool per second per instance,
    /// PoE constant. Worked example: 1100 Life × 0.02 = 22.
    #[test]
    fn life_leech_instance_rate_breakdown_walks_pool_and_rate() {
        let mut env = Env::default();
        env.output.set("Life", 1100.0);
        env.output.set("LifeLeechInstanceRate", 22.0);
        let bd = derive_for(&env, "LifeLeechInstanceRate").unwrap();
        let labels: Vec<&str> = bd.steps.iter().map(|s| s.label.as_str()).collect();
        assert!(labels.contains(&"Life pool"));
        assert!(labels.contains(&"Per-instance rate"));
        assert!(labels.contains(&"Life leech instance rate"));

        let rate_step = bd
            .steps
            .iter()
            .find(|s| s.label == "Per-instance rate")
            .unwrap();
        assert!((rate_step.value.unwrap() - 0.02).abs() < 1e-9);
        assert!((bd.total - 22.0).abs() < 1e-9);
    }

    /// Issue #34 follow-up: ManaLeechInstanceRate walks the same
    /// Pool → 2% rate chain. Worked example: 360 Mana × 0.02 = 7.2.
    #[test]
    fn mana_leech_instance_rate_breakdown_walks_pool_and_rate() {
        let mut env = Env::default();
        env.output.set("Mana", 360.0);
        env.output.set("ManaLeechInstanceRate", 7.2);
        let bd = derive_for(&env, "ManaLeechInstanceRate").unwrap();
        let labels: Vec<&str> = bd.steps.iter().map(|s| s.label.as_str()).collect();
        assert!(labels.contains(&"Mana pool"));
        assert!(labels.contains(&"Per-instance rate"));
        assert!(labels.contains(&"Mana leech instance rate"));
        assert!((bd.total - 7.2).abs() < 1e-9);
    }

    /// Issue #34 follow-up: returns None when the pool is zero
    /// (any of the four per-instance breakdowns).
    #[test]
    fn leech_instance_breakdowns_skipped_when_no_pool() {
        let env = Env::default();
        for key in [
            "MaxLifeLeechInstance",
            "MaxManaLeechInstance",
            "LifeLeechInstanceRate",
            "ManaLeechInstanceRate",
        ] {
            assert!(
                derive_for(&env, key).is_none(),
                "expected None for {key} when pool is zero",
            );
        }
    }

    /// Issue #34 follow-up: MaxLifeLeechRate walks Pool → 20%
    /// cap → Final per-second rate. PoE caps total leech at 20% of
    /// max pool per second; the breakdown surfaces the constant +
    /// pool source so users understand what their leech-rate ceiling
    /// is. Worked example: 1100 Life × 0.20 = 220/sec.
    #[test]
    fn max_life_leech_rate_breakdown_walks_pool_and_cap() {
        let mut env = Env::default();
        env.output.set("Life", 1100.0);
        env.output.set("MaxLifeLeechRate", 220.0);
        let bd = derive_for(&env, "MaxLifeLeechRate").unwrap();
        let labels: Vec<&str> = bd.steps.iter().map(|s| s.label.as_str()).collect();
        assert!(labels.contains(&"Life pool"));
        assert!(labels.contains(&"Cap"));
        assert!(labels.contains(&"Max life leech rate"));

        let pool = bd.steps.iter().find(|s| s.label == "Life pool").unwrap();
        assert!((pool.value.unwrap() - 1100.0).abs() < 1e-9);

        let cap = bd.steps.iter().find(|s| s.label == "Cap").unwrap();
        // 20% PoE constant.
        assert!((cap.value.unwrap() - 0.20).abs() < 1e-9);

        assert!((bd.total - 220.0).abs() < 1e-9);
    }

    /// Issue #34 follow-up: MaxManaLeechRate walks the same Pool →
    /// 20% cap → Final chain. Worked example: 360 Mana × 0.20 = 72/sec.
    #[test]
    fn max_mana_leech_rate_breakdown_walks_pool_and_cap() {
        let mut env = Env::default();
        env.output.set("Mana", 360.0);
        env.output.set("MaxManaLeechRate", 72.0);
        let bd = derive_for(&env, "MaxManaLeechRate").unwrap();
        let labels: Vec<&str> = bd.steps.iter().map(|s| s.label.as_str()).collect();
        assert!(labels.contains(&"Mana pool"));
        assert!(labels.contains(&"Cap"));
        assert!(labels.contains(&"Max mana leech rate"));
        assert!((bd.total - 72.0).abs() < 1e-9);
    }

    /// Issue #34 follow-up: returns None when the pool is zero so
    /// the panel falls back to the generic mods view rather than
    /// rendering 0/sec.
    #[test]
    fn max_leech_rate_breakdown_skipped_when_no_pool() {
        let env = Env::default();
        assert!(
            derive_for(&env, "MaxLifeLeechRate").is_none(),
            "expected None when Life is zero",
        );
        assert!(
            derive_for(&env, "MaxManaLeechRate").is_none(),
            "expected None when Mana is zero",
        );
    }

    /// Issue #34 follow-up: LowestOfMaximumLifeAndMaximumMana walks
    /// Life → Mana → min, flagging the source pool. Used by some
    /// unique mods (e.g. Mind over Matter follow-ons) and surfaced
    /// because it's a "which sustain pool dominates" question users
    /// genuinely ask. Worked example: 1100 Life + 360 Mana → 360
    /// (Mana).
    #[test]
    fn lowest_pool_breakdown_picks_min_and_flags_winner() {
        let mut env = Env::default();
        env.output.set("Life", 1100.0);
        env.output.set("Mana", 360.0);
        env.output.set("LowestOfMaximumLifeAndMaximumMana", 360.0);
        let bd = derive_for(&env, "LowestOfMaximumLifeAndMaximumMana").unwrap();
        let labels: Vec<&str> = bd.steps.iter().map(|s| s.label.as_str()).collect();
        assert!(labels.contains(&"Life pool"));
        assert!(labels.contains(&"Mana pool"));
        assert!(labels.contains(&"Lowest of life and mana"));

        let lowest = bd
            .steps
            .iter()
            .find(|s| s.label == "Lowest of life and mana")
            .unwrap();
        assert!(
            lowest
                .explain
                .as_deref()
                .is_some_and(|e| e.contains("Mana")),
            "expected Mana in winner explain, got {:?}",
            lowest.explain
        );
        assert!((bd.total - 360.0).abs() < 1e-9);
    }

    /// Issue #34 follow-up: ties pick Life — matches PoB's
    /// `life.min(mana)` chain. Worked example: 600 Life + 600 Mana →
    /// 600 (Life).
    #[test]
    fn lowest_pool_breakdown_breaks_ties_to_life() {
        let mut env = Env::default();
        env.output.set("Life", 600.0);
        env.output.set("Mana", 600.0);
        env.output.set("LowestOfMaximumLifeAndMaximumMana", 600.0);
        let bd = derive_for(&env, "LowestOfMaximumLifeAndMaximumMana").unwrap();
        let lowest = bd
            .steps
            .iter()
            .find(|s| s.label == "Lowest of life and mana")
            .unwrap();
        assert!(
            lowest
                .explain
                .as_deref()
                .is_some_and(|e| e.contains("Life")),
            "expected Life to win the tie, got {:?}",
            lowest.explain
        );
    }

    /// Issue #34 follow-up: returns None when both pools are zero
    /// (no character loaded yet).
    #[test]
    fn lowest_pool_breakdown_skipped_when_no_pools() {
        let env = Env::default();
        assert!(
            derive_for(&env, "LowestOfMaximumLifeAndMaximumMana").is_none(),
            "expected None when both pools are zero",
        );
    }

    /// Issue #34 follow-up: LowestAttribute walks each attribute and
    /// flags which one is the minimum. Used by `LowestAttribute`
    /// trigger mods (e.g. unique boots, certain timeless jewels).
    /// Worked example: 80 Str + 50 Dex + 60 Int → Lowest = 50 (Dex).
    #[test]
    fn lowest_attribute_breakdown_picks_min_and_flags_winner() {
        let mut env = Env::default();
        env.output.set("Strength", 80.0);
        env.output.set("Dexterity", 50.0);
        env.output.set("Intelligence", 60.0);
        env.output.set("LowestAttribute", 50.0);
        let bd = derive_for(&env, "LowestAttribute").unwrap();
        let labels: Vec<&str> = bd.steps.iter().map(|s| s.label.as_str()).collect();
        assert!(labels.contains(&"Strength"));
        assert!(labels.contains(&"Dexterity"));
        assert!(labels.contains(&"Intelligence"));
        assert!(labels.contains(&"Lowest attribute"));

        // The winner step's explain should mention the source attribute.
        let lowest = bd
            .steps
            .iter()
            .find(|s| s.label == "Lowest attribute")
            .unwrap();
        assert!(
            lowest
                .explain
                .as_deref()
                .is_some_and(|e| e.contains("Dexterity")),
            "expected Dexterity in winner explain, got {:?}",
            lowest.explain
        );
        assert!((bd.total - 50.0).abs() < 1e-9);
    }

    /// Issue #34 follow-up: ties between two attributes pick the
    /// first listed in (Strength, Dexterity, Intelligence) order.
    /// Worked example: 70 Str + 70 Dex + 100 Int → Lowest = 70 (Str).
    #[test]
    fn lowest_attribute_breakdown_breaks_ties_to_str() {
        let mut env = Env::default();
        env.output.set("Strength", 70.0);
        env.output.set("Dexterity", 70.0);
        env.output.set("Intelligence", 100.0);
        env.output.set("LowestAttribute", 70.0);
        let bd = derive_for(&env, "LowestAttribute").unwrap();
        let lowest = bd
            .steps
            .iter()
            .find(|s| s.label == "Lowest attribute")
            .unwrap();
        assert!(
            lowest
                .explain
                .as_deref()
                .is_some_and(|e| e.contains("Strength")),
            "expected Strength to win the tie, got {:?}",
            lowest.explain
        );
    }

    /// Issue #34 follow-up: TotalAttr walks Strength + Dexterity +
    /// Intelligence → Total. The Calcs tab surfaced a single number
    /// for the sum even though each attribute already has its own
    /// breakdown — the chain back makes the relationship explicit.
    /// Worked example: 80 Str + 50 Dex + 60 Int = 190.
    #[test]
    fn total_attr_breakdown_sums_three_attributes() {
        let mut env = Env::default();
        env.output.set("Strength", 80.0);
        env.output.set("Dexterity", 50.0);
        env.output.set("Intelligence", 60.0);
        env.output.set("TotalAttr", 190.0);
        let bd = derive_for(&env, "TotalAttr").unwrap();
        let labels: Vec<&str> = bd.steps.iter().map(|s| s.label.as_str()).collect();
        assert!(labels.contains(&"Strength"));
        assert!(labels.contains(&"Dexterity"));
        assert!(labels.contains(&"Intelligence"));
        assert!(labels.contains(&"Total attributes"));

        let str_step = bd.steps.iter().find(|s| s.label == "Strength").unwrap();
        assert!((str_step.value.unwrap() - 80.0).abs() < 1e-9);

        assert!((bd.total - 190.0).abs() < 1e-9);
    }

    /// Issue #34 follow-up: TotalAttr breakdown still renders when
    /// only one attribute is non-zero — the zero attributes still
    /// appear in the chain so the structure stays consistent across
    /// builds.
    #[test]
    fn total_attr_breakdown_renders_with_partial_attributes() {
        let mut env = Env::default();
        env.output.set("Strength", 100.0);
        env.output.set("TotalAttr", 100.0);
        let bd = derive_for(&env, "TotalAttr").unwrap();
        let labels: Vec<&str> = bd.steps.iter().map(|s| s.label.as_str()).collect();
        assert!(labels.contains(&"Strength"));
        assert!(labels.contains(&"Dexterity"));
        assert!(labels.contains(&"Intelligence"));

        let dex_step = bd.steps.iter().find(|s| s.label == "Dexterity").unwrap();
        assert!((dex_step.value.unwrap() - 0.0).abs() < 1e-9);
        assert!((bd.total - 100.0).abs() < 1e-9);
    }

    /// Issue #34 follow-up: LifeUnreserved walks Pool → Reserved →
    /// Unreserved. PoB shows the exact arithmetic so reservation
    /// builds (Auras, Heralds) can see how each aura eats into the
    /// pool. Worked example: 1100 Life - 350 Reserved = 750
    /// Unreserved.
    #[test]
    fn life_unreserved_breakdown_walks_pool_minus_reserved() {
        let mut env = Env::default();
        env.output.set("Life", 1100.0);
        env.output.set("LifeReserved", 350.0);
        env.output.set("LifeReservedPercent", 31.8);
        env.output.set("LifeUnreserved", 750.0);
        env.output.set("LifeUnreservedPercent", 68.2);
        let bd = derive_for(&env, "LifeUnreserved").unwrap();
        let labels: Vec<&str> = bd.steps.iter().map(|s| s.label.as_str()).collect();
        assert!(labels.contains(&"Life pool"));
        assert!(labels.contains(&"Reserved"));
        assert!(labels.contains(&"Unreserved"));

        let pool = bd.steps.iter().find(|s| s.label == "Life pool").unwrap();
        assert!((pool.value.unwrap() - 1100.0).abs() < 1e-9);

        let reserved = bd.steps.iter().find(|s| s.label == "Reserved").unwrap();
        assert!((reserved.value.unwrap() - 350.0).abs() < 1e-9);

        assert!((bd.total - 750.0).abs() < 1e-9);
    }

    /// Issue #34 follow-up: ManaUnreserved walks the same Pool →
    /// Reserved → Unreserved chain. Worked example: 360 Mana - 145
    /// Reserved (Discipline + Determination on a Witch) = 215
    /// Unreserved.
    #[test]
    fn mana_unreserved_breakdown_walks_pool_minus_reserved() {
        let mut env = Env::default();
        env.output.set("Mana", 360.0);
        env.output.set("ManaReserved", 145.0);
        env.output.set("ManaReservedPercent", 40.3);
        env.output.set("ManaUnreserved", 215.0);
        env.output.set("ManaUnreservedPercent", 59.7);
        let bd = derive_for(&env, "ManaUnreserved").unwrap();
        let labels: Vec<&str> = bd.steps.iter().map(|s| s.label.as_str()).collect();
        assert!(labels.contains(&"Mana pool"));
        assert!(labels.contains(&"Reserved"));
        assert!(labels.contains(&"Unreserved"));
        assert!((bd.total - 215.0).abs() < 1e-9);
    }

    /// Issue #34 follow-up: with no Life pool the dispatch returns
    /// None — the panel falls back rather than rendering 0/0.
    #[test]
    fn life_unreserved_breakdown_skipped_when_no_pool() {
        let env = Env::default();
        assert!(
            derive_for(&env, "LifeUnreserved").is_none(),
            "expected None when Life is zero",
        );
    }

    /// Issue #34 follow-up: AreaOfEffectRadius walks the
    /// PoB formula:
    ///
    /// Issue #34 follow-up: ProjectileCount breakdown. PoB derives
    /// Issue #34 follow-up: MainSkillEnemyEffectiveResist surfaces
    /// the post-penetration enemy resist value used by the damage
    /// chain. PoB derives it as `(enemy_resist_raw - elem_pen)`
    /// clamped to [-200, 95]. The configured enemy resist + the
    /// `<Element>Penetration` mods aren't both stored as output
    /// keys, so the breakdown shows the final value plus the cap
    /// rationale. Worked example: 75% raw - 10% pen = 65%.
    #[test]
    fn main_skill_enemy_effective_resist_breakdown_shows_clamp_and_value() {
        let mut env = Env::default();
        env.output.set("MainSkillEnemyEffectiveResist", 65.0);
        let bd = derive_for(&env, "MainSkillEnemyEffectiveResist").unwrap();
        let labels: Vec<&str> = bd.steps.iter().map(|s| s.label.as_str()).collect();
        assert!(labels.contains(&"Cap range"));
        assert!(labels.contains(&"Effective enemy resist"));

        let final_step = bd.steps.last().unwrap();
        assert!(
            final_step
                .explain
                .as_deref()
                .is_some_and(|e| e.contains("penetration") || e.contains("clamp")),
            "expected penetration / clamp note in explain, got {:?}",
            final_step.explain
        );
        assert!((bd.total - 65.0).abs() < 1e-9);
    }

    /// Issue #34 follow-up: returns None when the enemy effective
    /// resist is zero (no skill loaded, or non-elemental hit).
    #[test]
    fn main_skill_enemy_effective_resist_breakdown_skipped_when_zero() {
        let env = Env::default();
        assert!(
            derive_for(&env, "MainSkillEnemyEffectiveResist").is_none(),
            "expected None when MainSkillEnemyEffectiveResist is zero",
        );
    }

    /// Issue #34 follow-up: FireMin / FireMax surface the post-mod
    /// per-element hit bounds. PoB derives them through the same
    /// `base × mult` chain as MainSkillHit{Min,Max}, just with the
    /// per-element `<Element>{Min,Max}Base` raw skill values. The
    /// breakdown links back to the multiplier source.
    #[test]
    fn fire_min_breakdown_walks_base_and_multiplier() {
        let mut env = Env::default();
        env.output.set("FireMinBase", 100.0);
        env.output.set("FireMin", 220.0);
        let bd = derive_for(&env, "FireMin").unwrap();
        let labels: Vec<&str> = bd.steps.iter().map(|s| s.label.as_str()).collect();
        assert!(labels.contains(&"Base min"));
        assert!(labels.contains(&"Multiplier"));
        assert!(labels.contains(&"Fire min"));

        let mult = bd.steps.iter().find(|s| s.label == "Multiplier").unwrap();
        assert!((mult.value.unwrap() - 2.20).abs() < 1e-6);
        assert!((bd.total - 220.0).abs() < 1e-9);
    }

    /// Issue #34 follow-up: ColdMax / LightningMax / etc. share
    /// the same shape. Worked example: 200 base × 2.20 = 440.
    #[test]
    fn other_element_max_breakdowns_share_shape() {
        for elem in ["Cold", "Lightning", "Physical", "Chaos"] {
            let mut env = Env::default();
            env.output.set(format!("{elem}MaxBase"), 200.0);
            env.output.set(format!("{elem}Max"), 440.0);
            let bd = derive_for(&env, &format!("{elem}Max"))
                .unwrap_or_else(|| panic!("{elem}Max: dispatch missing"));
            assert!((bd.total - 440.0).abs() < 1e-9, "{elem}: wrong total");
        }
    }

    /// Issue #34 follow-up: per-element MinBase / MaxBase surface
    /// the raw per-level base damage (single-row breakdown — same
    /// shape as MainSkillBase{Min,Max}).
    #[test]
    fn fire_max_base_breakdown_shows_raw_base() {
        let mut env = Env::default();
        env.output.set("FireMaxBase", 200.0);
        let bd = derive_for(&env, "FireMaxBase").unwrap();
        let labels: Vec<&str> = bd.steps.iter().map(|s| s.label.as_str()).collect();
        assert!(labels.contains(&"Fire max base"));
        assert!((bd.total - 200.0).abs() < 1e-9);
    }

    /// Issue #34 follow-up: returns None when the element wasn't
    /// the active skill's damage type.
    #[test]
    fn element_min_max_breakdowns_skipped_when_zero() {
        let env = Env::default();
        for elem in ["Fire", "Cold", "Lightning", "Physical", "Chaos"] {
            for suffix in ["Min", "Max", "MinBase", "MaxBase"] {
                let key = format!("{elem}{suffix}");
                assert!(
                    derive_for(&env, &key).is_none(),
                    "expected None for {key} when zero",
                );
            }
        }
    }

    /// Issue #34 follow-up: FireHitAverage walks Min → Max →
    /// average. PoB exposes per-element `<Element>Min` /
    /// `<Element>Max` (post-mod hit values) and `<Element>HitAverage`
    /// = (Min + Max) / 2 for each of the five damage types
    /// (`perform_skill_dps`). The Calcs panel surfaces the average;
    /// the breakdown shows the bounds it's averaging.
    /// Worked example: FireMin 220 + FireMax 440 → average 330.
    #[test]
    fn fire_hit_average_breakdown_walks_min_max_to_average() {
        let mut env = Env::default();
        env.output.set("FireMin", 220.0);
        env.output.set("FireMax", 440.0);
        env.output.set("FireHitAverage", 330.0);
        let bd = derive_for(&env, "FireHitAverage").unwrap();
        let labels: Vec<&str> = bd.steps.iter().map(|s| s.label.as_str()).collect();
        assert!(labels.contains(&"Min"));
        assert!(labels.contains(&"Max"));
        assert!(labels.contains(&"Fire hit average"));
        assert!((bd.total - 330.0).abs() < 1e-9);
    }

    /// Issue #34 follow-up: ChaosHitAverage / PhysicalHitAverage etc
    /// share the same shape.
    #[test]
    fn other_element_hit_average_breakdowns_share_shape() {
        for elem in ["Cold", "Lightning", "Physical", "Chaos"] {
            let mut env = Env::default();
            env.output.set(format!("{elem}Min"), 100.0);
            env.output.set(format!("{elem}Max"), 200.0);
            env.output.set(format!("{elem}HitAverage"), 150.0);
            let bd = derive_for(&env, &format!("{elem}HitAverage"))
                .unwrap_or_else(|| panic!("{elem}HitAverage: dispatch missing"));
            assert!((bd.total - 150.0).abs() < 1e-9, "{elem}: wrong total");
        }
    }

    /// Issue #34 follow-up: returns None when the element wasn't
    /// the active skill's damage type (no per-element outputs).
    #[test]
    fn element_hit_average_breakdown_skipped_when_zero() {
        let env = Env::default();
        for elem in ["Fire", "Cold", "Lightning", "Physical", "Chaos"] {
            assert!(
                derive_for(&env, &format!("{elem}HitAverage")).is_none(),
                "expected None for {elem}HitAverage when zero",
            );
        }
    }

    /// Issue #34 follow-up: LifeReserved walks the complementary
    /// arithmetic to `LifeUnreserved` — `pool − unreserved`. The
    /// flat / percent reservation contributions aren't both on
    /// `env.output` (perform_reservations folds them into the
    /// totals during the active-aura sweep), so the breakdown
    /// surfaces the high-level subtraction with a back-link to the
    /// auras / heralds. Worked example: 1100 pool - 750 unreserved
    /// = 350 reserved.
    #[test]
    fn life_reserved_breakdown_walks_pool_minus_unreserved() {
        let mut env = Env::default();
        env.output.set("Life", 1100.0);
        env.output.set("LifeUnreserved", 750.0);
        env.output.set("LifeReserved", 350.0);
        env.output.set("LifeReservedPercent", 31.8);
        let bd = derive_for(&env, "LifeReserved").unwrap();
        let labels: Vec<&str> = bd.steps.iter().map(|s| s.label.as_str()).collect();
        assert!(labels.contains(&"Life pool"));
        assert!(labels.contains(&"Unreserved"));
        assert!(labels.contains(&"Life reserved"));
        assert!((bd.total - 350.0).abs() < 1e-9);
    }

    /// Issue #34 follow-up: ManaReserved walks the same shape.
    #[test]
    fn mana_reserved_breakdown_walks_pool_minus_unreserved() {
        let mut env = Env::default();
        env.output.set("Mana", 360.0);
        env.output.set("ManaUnreserved", 215.0);
        env.output.set("ManaReserved", 145.0);
        env.output.set("ManaReservedPercent", 40.3);
        let bd = derive_for(&env, "ManaReserved").unwrap();
        let labels: Vec<&str> = bd.steps.iter().map(|s| s.label.as_str()).collect();
        assert!(labels.contains(&"Mana pool"));
        assert!(labels.contains(&"Mana reserved"));
        assert!((bd.total - 145.0).abs() < 1e-9);
    }

    /// Issue #34 follow-up: both Reserved breakdowns return None
    /// when the pool is zero.
    #[test]
    fn reserved_breakdowns_skipped_when_no_pool() {
        let env = Env::default();
        for key in ["LifeReserved", "ManaReserved"] {
            assert!(
                derive_for(&env, key).is_none(),
                "expected None for {key} when pool is zero",
            );
        }
    }

    /// Issue #34 follow-up: LifeReservedPercent walks the
    /// reservation arithmetic — `reserved / pool × 100`. The Calcs
    /// tab surfaces the percent next to the absolute reserved
    /// number; the breakdown shows the division explicitly so
    /// users can see what's eating their pool. Worked example:
    /// 350 reserved / 1100 pool × 100 = 31.8%.
    #[test]
    fn life_reserved_percent_breakdown_walks_reservation_arithmetic() {
        let mut env = Env::default();
        env.output.set("Life", 1100.0);
        env.output.set("LifeReserved", 350.0);
        env.output.set("LifeReservedPercent", 31.8);
        let bd = derive_for(&env, "LifeReservedPercent").unwrap();
        let labels: Vec<&str> = bd.steps.iter().map(|s| s.label.as_str()).collect();
        assert!(labels.contains(&"Life pool"));
        assert!(labels.contains(&"Reserved"));
        assert!(labels.contains(&"Life reserved (%)"));

        let pool = bd.steps.iter().find(|s| s.label == "Life pool").unwrap();
        assert!((pool.value.unwrap() - 1100.0).abs() < 1e-9);

        let reserved = bd.steps.iter().find(|s| s.label == "Reserved").unwrap();
        assert!((reserved.value.unwrap() - 350.0).abs() < 1e-9);

        assert!((bd.total - 31.8).abs() < 1e-6);
    }

    /// Issue #34 follow-up: ManaUnreservedPercent walks the
    /// complementary ratio (`unreserved / pool × 100`). Worked
    /// example: 215 unreserved / 360 pool = 59.7%.
    #[test]
    fn mana_unreserved_percent_breakdown_walks_unreserved_ratio() {
        let mut env = Env::default();
        env.output.set("Mana", 360.0);
        env.output.set("ManaUnreserved", 215.0);
        env.output.set("ManaUnreservedPercent", 59.7);
        let bd = derive_for(&env, "ManaUnreservedPercent").unwrap();
        let labels: Vec<&str> = bd.steps.iter().map(|s| s.label.as_str()).collect();
        assert!(labels.contains(&"Mana pool"));
        assert!(labels.contains(&"Unreserved"));
        assert!(labels.contains(&"Mana unreserved (%)"));
        assert!((bd.total - 59.7).abs() < 1e-6);
    }

    /// Issue #34 follow-up: all four reservation-percent
    /// breakdowns return None when the pool is zero.
    #[test]
    fn reservation_percent_breakdowns_skipped_when_no_pool() {
        let env = Env::default();
        for key in [
            "LifeReservedPercent",
            "ManaReservedPercent",
            "LifeUnreservedPercent",
            "ManaUnreservedPercent",
        ] {
            assert!(
                derive_for(&env, key).is_none(),
                "expected None for {key} when pool is zero",
            );
        }
    }

    /// Issue #34 follow-up: AoERadius walks the base AoE radius
    /// from skill stats + `AreaOfEffect` BASE mod sum. PoB derives
    /// it as `skill_base + Σ BASE("AreaOfEffect")` (perform_skill_dps).
    /// Worked example: skill base 22 + 5 from a passive = 27 base
    /// radius (which then feeds AreaOfEffectRadius via the sqrt
    /// scaling).
    #[test]
    fn aoe_radius_base_breakdown_walks_skill_base_and_adders() {
        let mut env = Env::default();
        env.mod_db
            .add(Mod::base("AreaOfEffect", 5.0).with_source(Source::Tree));
        env.output.set("AoERadius", 27.0);
        let bd = derive_for(&env, "AoERadius").unwrap();
        let labels: Vec<&str> = bd.steps.iter().map(|s| s.label.as_str()).collect();
        assert!(labels.contains(&"BASE adders"));
        assert!(labels.contains(&"Base radius"));

        let base = bd.steps.iter().find(|s| s.label == "BASE adders").unwrap();
        assert!((base.value.unwrap() - 5.0).abs() < 1e-9);
        assert!((bd.total - 27.0).abs() < 1e-9);
    }

    /// Issue #34 follow-up: AoERadius collapses to a single row
    /// (base only) when there are no `AreaOfEffect` BASE mods.
    #[test]
    fn aoe_radius_base_breakdown_collapses_when_no_base_mods() {
        let mut env = Env::default();
        env.output.set("AoERadius", 22.0);
        let bd = derive_for(&env, "AoERadius").unwrap();
        let labels: Vec<&str> = bd.steps.iter().map(|s| s.label.as_str()).collect();
        assert!(labels.contains(&"Base radius"));
        assert!(!labels.contains(&"BASE adders"));
        assert!((bd.total - 22.0).abs() < 1e-9);
    }

    /// Issue #34 follow-up: returns None when no AoE skill is loaded
    /// (AoERadius is zero).
    #[test]
    fn aoe_radius_base_breakdown_skipped_when_zero() {
        let env = Env::default();
        assert!(
            derive_for(&env, "AoERadius").is_none(),
            "expected None when AoERadius is zero",
        );
    }

    /// Issue #34 follow-up: LifeFlaskRecovery walks each flask
    /// slot's per-flask LifeRecovery and surfaces the max across
    /// slots. PoB takes the max because the user only fires one
    /// flask at a time. Worked example: Flask1 350, Flask2 540,
    /// Flask4 320 → max 540 (Flask 2).
    #[test]
    fn life_flask_recovery_breakdown_picks_max_across_slots() {
        let mut env = Env::default();
        env.output.set("Flask1LifeRecovery", 350.0);
        env.output.set("Flask2LifeRecovery", 540.0);
        env.output.set("Flask4LifeRecovery", 320.0);
        env.output.set("LifeFlaskRecovery", 540.0);
        let bd = derive_for(&env, "LifeFlaskRecovery").unwrap();
        let labels: Vec<&str> = bd.steps.iter().map(|s| s.label.as_str()).collect();
        assert!(labels.contains(&"Flask 1"));
        assert!(labels.contains(&"Flask 2"));
        assert!(labels.contains(&"Flask 3"));
        assert!(labels.contains(&"Flask 4"));
        assert!(labels.contains(&"Flask 5"));
        assert!(labels.contains(&"Life flask recovery"));

        let final_step = bd.steps.last().unwrap();
        assert!(
            final_step
                .explain
                .as_deref()
                .is_some_and(|e| e.contains("Flask 2") && e.contains("max")),
            "expected 'max from Flask 2' note, got {:?}",
            final_step.explain
        );
        assert!((bd.total - 540.0).abs() < 1e-9);
    }

    /// Issue #34 follow-up: ManaFlaskRecovery walks the same per-
    /// slot pick. Worked example: Flask1 200 mana, Flask3 480 →
    /// max 480 (Flask 3).
    #[test]
    fn mana_flask_recovery_breakdown_picks_max_across_slots() {
        let mut env = Env::default();
        env.output.set("Flask1ManaRecovery", 200.0);
        env.output.set("Flask3ManaRecovery", 480.0);
        env.output.set("ManaFlaskRecovery", 480.0);
        let bd = derive_for(&env, "ManaFlaskRecovery").unwrap();
        let labels: Vec<&str> = bd.steps.iter().map(|s| s.label.as_str()).collect();
        assert!(labels.contains(&"Mana flask recovery"));
        assert!((bd.total - 480.0).abs() < 1e-9);
    }

    /// Issue #34 follow-up: returns None when the user has no flask
    /// of that type equipped (LifeFlaskRecovery / ManaFlaskRecovery
    /// = 0 means perform.rs didn't write them).
    #[test]
    fn flask_recovery_breakdown_skipped_when_no_flask() {
        let env = Env::default();
        for key in ["LifeFlaskRecovery", "ManaFlaskRecovery"] {
            assert!(
                derive_for(&env, key).is_none(),
                "expected None for {key} when no flask is equipped",
            );
        }
    }

    /// Issue #34 follow-up: EnemyPhysReduction walks the armour
    /// formula: PoB derives it as `armour / (armour + 5 × raw)`
    /// capped at 90% (the DamageReductionMax constant). Surfaces
    /// the configured enemy armour from the boss preset, the raw
    /// per-hit damage that drives the soak, and the cap.
    ///
    /// Worked example: 25k enemy armour vs 660 raw → 25000 / (25000
    /// + 5 × 660) = 88.4% reduction.
    #[test]
    fn enemy_phys_reduction_breakdown_walks_armour_formula() {
        let mut env = Env::default();
        env.output.set("MainSkillAverageHit", 660.0);
        env.output.set("EnemyPhysReduction", 88.4);
        let bd = derive_for(&env, "EnemyPhysReduction").unwrap();
        let labels: Vec<&str> = bd.steps.iter().map(|s| s.label.as_str()).collect();
        assert!(labels.contains(&"Raw hit"));
        assert!(labels.contains(&"Cap"));
        assert!(labels.contains(&"Enemy physical damage reduction"));

        let cap = bd.steps.iter().find(|s| s.label == "Cap").unwrap();
        assert!((cap.value.unwrap() - 90.0).abs() < 1e-9);
        assert!((bd.total - 88.4).abs() < 1e-6);
    }

    /// Issue #34 follow-up: returns None when reduction is zero
    /// (non-physical hit, or no enemy armour).
    #[test]
    fn enemy_phys_reduction_breakdown_skipped_when_zero() {
        let env = Env::default();
        assert!(
            derive_for(&env, "EnemyPhysReduction").is_none(),
            "expected None when EnemyPhysReduction is zero",
        );
    }

    /// Issue #34 follow-up: MainSkillLevel surfaces the active gem
    /// level (clamped 1-40 per `perform_skill_dps`). Calcs panel
    /// rows that depend on level (base damage, mana cost, area)
    /// link back to this row. Surfacing the value as a single-step
    /// breakdown keeps the click-through chain consistent.
    #[test]
    fn main_skill_level_breakdown_shows_gem_level() {
        let mut env = Env::default();
        env.output.set("MainSkillLevel", 20.0);
        let bd = derive_for(&env, "MainSkillLevel").unwrap();
        let labels: Vec<&str> = bd.steps.iter().map(|s| s.label.as_str()).collect();
        assert!(labels.contains(&"Main skill level"));

        let final_step = bd.steps.last().unwrap();
        assert!(
            final_step
                .explain
                .as_deref()
                .is_some_and(|e| e.contains("gem") || e.contains("clamp")),
            "expected gem-level hint in explain, got {:?}",
            final_step.explain
        );
        assert!((bd.total - 20.0).abs() < 1e-9);
    }

    /// Issue #34 follow-up: returns None when no skill is loaded
    /// (MainSkillLevel = 0).
    #[test]
    fn main_skill_level_breakdown_skipped_when_no_skill() {
        let env = Env::default();
        assert!(
            derive_for(&env, "MainSkillLevel").is_none(),
            "expected None when MainSkillLevel is zero",
        );
    }

    /// Issue #34 follow-up: WeaponRangeMetre converts WeaponRange
    /// from engine units to metres (`/ 10`), same shape as
    /// AreaOfEffectRadiusMetres. Surfacing the chain lets users see
    /// the engine-unit value next to the metres value without
    /// context-switching. Worked example: 8 engine units → 0.8 m.
    #[test]
    fn weapon_range_metre_breakdown_walks_units_to_metres() {
        let mut env = Env::default();
        env.output.set("WeaponRange", 8.0);
        env.output.set("WeaponRangeMetre", 0.8);
        let bd = derive_for(&env, "WeaponRangeMetre").unwrap();
        let labels: Vec<&str> = bd.steps.iter().map(|s| s.label.as_str()).collect();
        assert!(labels.contains(&"Range (engine units)"));
        assert!(labels.contains(&"Weapon range (metres)"));

        let units = bd
            .steps
            .iter()
            .find(|s| s.label == "Range (engine units)")
            .unwrap();
        assert!((units.value.unwrap() - 8.0).abs() < 1e-9);
        assert!((bd.total - 0.8).abs() < 1e-6);
    }

    /// Issue #34 follow-up: returns None when WeaponRange is zero
    /// (no character loaded yet).
    #[test]
    fn weapon_range_metre_breakdown_skipped_when_no_range() {
        let env = Env::default();
        assert!(
            derive_for(&env, "WeaponRangeMetre").is_none(),
            "expected None when WeaponRange is zero",
        );
    }

    /// Issue #34 follow-up: FireResist (uncapped) walks BASE +
    /// umbrella + level penalty → Final without the cap step. The
    /// capped value is exposed separately as FireResistTotal (with
    /// its own breakdown showing the cap clamp); the uncapped row
    /// is what shows users their "overcap" — e.g. a 90% sum capped
    /// at 75% means 15% wasted resist.
    ///
    /// Worked example: 80 from BASE - 60 level penalty = 20 final
    /// uncapped fire resist.
    #[test]
    fn fire_resist_uncapped_breakdown_walks_base_and_penalty() {
        let mut env = Env::default();
        env.mod_db
            .add(Mod::base("FireResist", 80.0).with_source(Source::Tree));
        env.output.set("FireResist", 20.0);
        let bd = derive_for(&env, "FireResist").unwrap();
        let labels: Vec<&str> = bd.steps.iter().map(|s| s.label.as_str()).collect();
        assert!(labels.contains(&"Fire Resist BASE"));
        assert!(labels.contains(&"Level penalty"));
        assert!(labels.contains(&"Fire Resist (uncapped)"));
        // None of the cap-related labels should leak into the
        // uncapped breakdown.
        assert!(!labels.contains(&"Cap"));
        assert!((bd.total - 20.0).abs() < 1e-9);
    }

    /// Issue #34 follow-up: ColdResist / LightningResist /
    /// ChaosResist all share the same uncapped chain.
    #[test]
    fn other_resist_uncapped_breakdowns_walk_same_chain() {
        for elem in ["Cold", "Lightning", "Chaos"] {
            let mut env = Env::default();
            env.output.set(format!("{elem}Resist"), 75.0);
            let bd = derive_for(&env, &format!("{elem}Resist"))
                .unwrap_or_else(|| panic!("{elem}Resist: dispatch missing"));
            let labels: Vec<&str> = bd.steps.iter().map(|s| s.label.as_str()).collect();
            assert!(
                labels.contains(&format!("{elem} Resist (uncapped)").as_str()),
                "{elem}: missing final label"
            );
            assert!((bd.total - 75.0).abs() < 1e-9);
        }
    }

    /// Issue #34 follow-up: returns None when the resist is zero
    /// (no character loaded yet).
    #[test]
    fn uncapped_resist_breakdown_skipped_when_zero() {
        let env = Env::default();
        for key in ["FireResist", "ColdResist", "LightningResist", "ChaosResist"] {
            assert!(
                derive_for(&env, key).is_none(),
                "expected None for {key} when zero",
            );
        }
    }

    /// Issue #34 follow-up: MainSkillBaseMin / MainSkillBaseMax
    /// surface the raw per-level base damage value from the skill
    /// stats — what the gem provides before player mods. Surfacing
    /// the value as a single-step breakdown lets users see the
    /// gem's contribution alongside the BaseAdd / Inc / More chain
    /// in MainSkillHit{Min,Max}.
    ///
    /// This is intentionally a single-row breakdown — the value is
    /// raw skill data, not derived from anything we can decompose
    /// further. Surfacing it lets the click-through chain stay
    /// consistent (every Calcs row gets a breakdown).
    #[test]
    fn main_skill_base_min_breakdown_shows_raw_base() {
        let mut env = Env::default();
        env.output.set("MainSkillBaseMin", 100.0);
        let bd = derive_for(&env, "MainSkillBaseMin").unwrap();
        let labels: Vec<&str> = bd.steps.iter().map(|s| s.label.as_str()).collect();
        assert!(labels.contains(&"Base min"));

        let final_step = bd.steps.last().unwrap();
        assert!(
            final_step
                .explain
                .as_deref()
                .is_some_and(|e| e.contains("skill stats") || e.contains("raw")),
            "expected raw-skill-data hint in explain, got {:?}",
            final_step.explain
        );
        assert!((bd.total - 100.0).abs() < 1e-9);
    }

    /// Issue #34 follow-up: MainSkillBaseMax shows the raw upper
    /// bound. Worked example: 200 from skill stats.
    #[test]
    fn main_skill_base_max_breakdown_shows_raw_base() {
        let mut env = Env::default();
        env.output.set("MainSkillBaseMax", 200.0);
        let bd = derive_for(&env, "MainSkillBaseMax").unwrap();
        let labels: Vec<&str> = bd.steps.iter().map(|s| s.label.as_str()).collect();
        assert!(labels.contains(&"Base max"));
        assert!((bd.total - 200.0).abs() < 1e-9);
    }

    /// Issue #34 follow-up: both base outputs return None when no
    /// skill is loaded.
    #[test]
    fn main_skill_base_breakdowns_skipped_when_no_skill() {
        let env = Env::default();
        for key in ["MainSkillBaseMin", "MainSkillBaseMax"] {
            assert!(
                derive_for(&env, key).is_none(),
                "expected None for {key} when no skill is loaded",
            );
        }
    }

    /// Issue #34 follow-up: MainSkillHitMin walks Base × Multiplier
    /// → Final. PoB derives both bounds via the same multiplier
    /// (back-derivable from final/base). The breakdown shows the
    /// raw skill-base value alongside the composed mult so users
    /// can see what their increased / more / quality stack
    /// contributes. Worked example: 100 base × 2.20× mult = 220.
    #[test]
    fn main_skill_hit_min_breakdown_walks_base_and_mult() {
        let mut env = Env::default();
        env.output.set("MainSkillBaseMin", 100.0);
        env.output.set("MainSkillHitMin", 220.0);
        let bd = derive_for(&env, "MainSkillHitMin").unwrap();
        let labels: Vec<&str> = bd.steps.iter().map(|s| s.label.as_str()).collect();
        assert!(labels.contains(&"Base min"));
        assert!(labels.contains(&"Multiplier"));
        assert!(labels.contains(&"Hit min"));

        let base = bd.steps.iter().find(|s| s.label == "Base min").unwrap();
        assert!((base.value.unwrap() - 100.0).abs() < 1e-9);

        let mult = bd.steps.iter().find(|s| s.label == "Multiplier").unwrap();
        assert!((mult.value.unwrap() - 2.20).abs() < 1e-6);

        assert!((bd.total - 220.0).abs() < 1e-9);
    }

    /// Issue #34 follow-up: MainSkillHitMax walks the same shape
    /// against the max-bound base. Worked example: 200 base × 2.20×
    /// = 440.
    #[test]
    fn main_skill_hit_max_breakdown_walks_base_and_mult() {
        let mut env = Env::default();
        env.output.set("MainSkillBaseMax", 200.0);
        env.output.set("MainSkillHitMax", 440.0);
        let bd = derive_for(&env, "MainSkillHitMax").unwrap();
        let labels: Vec<&str> = bd.steps.iter().map(|s| s.label.as_str()).collect();
        assert!(labels.contains(&"Base max"));
        assert!(labels.contains(&"Multiplier"));
        assert!(labels.contains(&"Hit max"));
        assert!((bd.total - 440.0).abs() < 1e-9);
    }

    /// Issue #34 follow-up: returns None when no skill is loaded
    /// (no MainSkill base values populated).
    #[test]
    fn main_skill_hit_min_max_breakdowns_skipped_when_no_skill() {
        let env = Env::default();
        for key in ["MainSkillHitMin", "MainSkillHitMax"] {
            assert!(
                derive_for(&env, key).is_none(),
                "expected None for {key} when no skill is loaded",
            );
        }
    }

    /// Issue #34 follow-up: SecondMinimalMaximumHitTaken walks the
    /// five per-element max-hits, surfaces the smallest (excluded),
    /// and picks the second-smallest. PoB uses this on the defence
    /// panel as "next worst max hit" — what the build can take if
    /// the smallest element is mitigated. Worked example: Phys
    /// 1100, Fire 4400, Cold 4400, Lightning 4400, Chaos 1400 →
    /// smallest 1100 (Phys), second 1400 (Chaos).
    #[test]
    fn second_min_max_hit_breakdown_picks_second_smallest() {
        let mut env = Env::default();
        env.output.set("PhysicalMaximumHitTaken", 1100.0);
        env.output.set("FireMaximumHitTaken", 4400.0);
        env.output.set("ColdMaximumHitTaken", 4400.0);
        env.output.set("LightningMaximumHitTaken", 4400.0);
        env.output.set("ChaosMaximumHitTaken", 1400.0);
        env.output.set("SecondMinimalMaximumHitTaken", 1400.0);
        let bd = derive_for(&env, "SecondMinimalMaximumHitTaken").unwrap();
        let labels: Vec<&str> = bd.steps.iter().map(|s| s.label.as_str()).collect();
        assert!(labels.contains(&"Physical"));
        assert!(labels.contains(&"Fire"));
        assert!(labels.contains(&"Cold"));
        assert!(labels.contains(&"Lightning"));
        assert!(labels.contains(&"Chaos"));
        assert!(labels.contains(&"Second-smallest max hit"));

        let final_step = bd.steps.last().unwrap();
        assert!(
            final_step
                .explain
                .as_deref()
                .is_some_and(|e| e.contains("Chaos") && e.contains("Physical")),
            "expected smallest (Physical) and second (Chaos) in explain, got {:?}",
            final_step.explain
        );
        assert!((bd.total - 1400.0).abs() < 1e-9);
    }

    /// Issue #34 follow-up: returns None when no per-element max-hits
    /// exist (no character loaded yet).
    #[test]
    fn second_min_max_hit_breakdown_skipped_when_no_elements() {
        let env = Env::default();
        assert!(
            derive_for(&env, "SecondMinimalMaximumHitTaken").is_none(),
            "expected None when no per-element max-hits are set",
        );
    }

    /// Issue #34 follow-up: per-element MoMHitPool /
    /// ManaEffectiveLife outputs all collapse to the same `pool`
    /// sum in Phase 2 (no MoM yet, no per-element split). Surfacing
    /// the chain shows the pool components.
    #[test]
    fn fire_mom_hit_pool_breakdown_walks_pool_components() {
        let mut env = Env::default();
        env.output.set("Life", 800.0);
        env.output.set("EnergyShield", 200.0);
        env.output.set("Ward", 100.0);
        env.output.set("FireMoMHitPool", 1100.0);
        let bd = derive_for(&env, "FireMoMHitPool").unwrap();
        let labels: Vec<&str> = bd.steps.iter().map(|s| s.label.as_str()).collect();
        assert!(labels.contains(&"Life"));
        assert!(labels.contains(&"Energy shield"));
        assert!(labels.contains(&"Ward"));
        assert!(labels.contains(&"Fire MoM hit pool"));
        assert!((bd.total - 1100.0).abs() < 1e-9);
    }

    /// Issue #34 follow-up: shared (non-element-specific) MoM
    /// outputs use the same pool sum but no element prefix.
    #[test]
    fn shared_mom_hit_pool_breakdown_walks_pool_components() {
        let mut env = Env::default();
        env.output.set("Life", 1100.0);
        env.output.set("EnergyShield", 0.0);
        env.output.set("Ward", 0.0);
        env.output.set("sharedMoMHitPool", 1100.0);
        let bd = derive_for(&env, "sharedMoMHitPool").unwrap();
        let labels: Vec<&str> = bd.steps.iter().map(|s| s.label.as_str()).collect();
        assert!(labels.contains(&"Life"));
        assert!(labels.contains(&"Shared MoM hit pool"));
        assert!((bd.total - 1100.0).abs() < 1e-9);
    }

    /// Issue #34 follow-up: returns None for any of the per-element
    /// MoM outputs when the pool is zero.
    #[test]
    fn mom_pool_breakdown_skipped_when_no_pool() {
        let env = Env::default();
        for key in [
            "PhysicalMoMHitPool",
            "FireMoMHitPool",
            "ColdMoMHitPool",
            "LightningMoMHitPool",
            "ChaosMoMHitPool",
            "PhysicalManaEffectiveLife",
            "FireManaEffectiveLife",
            "ColdManaEffectiveLife",
            "LightningManaEffectiveLife",
            "ChaosManaEffectiveLife",
            "sharedMoMHitPool",
            "sharedManaEffectiveLife",
        ] {
            assert!(
                derive_for(&env, key).is_none(),
                "expected None for {key} when pool is zero",
            );
        }
    }

    /// Issue #34 follow-up: LifeHitPool / LifeRecoverable /
    /// StunThreshold all collapse to the Life output in Phase 2 (no
    /// recoup mods, no MoM split). Surfacing the alias relationship
    /// in the breakdown panel saves users from wondering why three
    /// rows show the same number.
    #[test]
    fn life_hit_pool_breakdown_aliases_life() {
        let mut env = Env::default();
        env.output.set("Life", 1100.0);
        env.output.set("LifeHitPool", 1100.0);
        let bd = derive_for(&env, "LifeHitPool").unwrap();
        let labels: Vec<&str> = bd.steps.iter().map(|s| s.label.as_str()).collect();
        assert!(labels.contains(&"Life"));
        assert!(labels.contains(&"Life hit pool"));
        let final_step = bd.steps.last().unwrap();
        assert!(
            final_step
                .explain
                .as_deref()
                .is_some_and(|e| e.contains("alias") || e.contains("Phase 2")),
            "expected alias hint in explain, got {:?}",
            final_step.explain
        );
        assert!((bd.total - 1100.0).abs() < 1e-9);
    }

    /// Issue #34 follow-up: LifeRecoverable shares the same alias
    /// shape as LifeHitPool — both equal Life in Phase 2.
    #[test]
    fn life_recoverable_breakdown_aliases_life() {
        let mut env = Env::default();
        env.output.set("Life", 800.0);
        env.output.set("LifeRecoverable", 800.0);
        let bd = derive_for(&env, "LifeRecoverable").unwrap();
        let labels: Vec<&str> = bd.steps.iter().map(|s| s.label.as_str()).collect();
        assert!(labels.contains(&"Life recoverable"));
        assert!((bd.total - 800.0).abs() < 1e-9);
    }

    /// Issue #34 follow-up: StunThreshold also = Life in Phase 2.
    #[test]
    fn stun_threshold_breakdown_aliases_life() {
        let mut env = Env::default();
        env.output.set("Life", 1500.0);
        env.output.set("StunThreshold", 1500.0);
        let bd = derive_for(&env, "StunThreshold").unwrap();
        let labels: Vec<&str> = bd.steps.iter().map(|s| s.label.as_str()).collect();
        assert!(labels.contains(&"Stun threshold"));
        assert!((bd.total - 1500.0).abs() < 1e-9);
    }

    /// Issue #34 follow-up: all three life-pool aliases return None
    /// when Life is zero (no character loaded yet).
    #[test]
    fn life_pool_alias_breakdowns_skipped_when_no_life() {
        let env = Env::default();
        for key in ["LifeHitPool", "LifeRecoverable", "StunThreshold"] {
            assert!(
                derive_for(&env, key).is_none(),
                "expected None for {key} when Life is zero",
            );
        }
    }

    /// Issue #34 follow-up: FireTotalHitPool walks Life + ES + Ward
    /// → Pool. PoB exposes per-element `<Element>TotalHitPool` /
    /// `<Element>TotalPool` outputs that all collapse to the same
    /// pool sum (Phase 2 — no MoM yet). Surfacing the chain shows
    /// the additive components so users can see what's contributing.
    /// Worked example: 1100 life + 0 ES + 0 ward → 1100 pool.
    #[test]
    fn fire_total_hit_pool_breakdown_walks_pool_components() {
        let mut env = Env::default();
        env.output.set("Life", 1100.0);
        env.output.set("EnergyShield", 0.0);
        env.output.set("Ward", 0.0);
        env.output.set("FireTotalHitPool", 1100.0);
        let bd = derive_for(&env, "FireTotalHitPool").unwrap();
        let labels: Vec<&str> = bd.steps.iter().map(|s| s.label.as_str()).collect();
        assert!(labels.contains(&"Life"));
        assert!(labels.contains(&"Energy shield"));
        assert!(labels.contains(&"Ward"));
        assert!(labels.contains(&"Fire total hit pool"));
        assert!((bd.total - 1100.0).abs() < 1e-9);
    }

    /// Issue #34 follow-up: ColdTotalPool / ChaosTotalPool / etc
    /// share the same chain, just under different output keys.
    /// Worked example: 800 life + 200 ES + 100 ward → 1100 pool.
    #[test]
    fn cold_total_pool_breakdown_sums_three_components() {
        let mut env = Env::default();
        env.output.set("Life", 800.0);
        env.output.set("EnergyShield", 200.0);
        env.output.set("Ward", 100.0);
        env.output.set("ColdTotalPool", 1100.0);
        let bd = derive_for(&env, "ColdTotalPool").unwrap();
        let labels: Vec<&str> = bd.steps.iter().map(|s| s.label.as_str()).collect();
        assert!(labels.contains(&"Cold total pool"));

        let es = bd
            .steps
            .iter()
            .find(|s| s.label == "Energy shield")
            .unwrap();
        assert!((es.value.unwrap() - 200.0).abs() < 1e-9);
        assert!((bd.total - 1100.0).abs() < 1e-9);
    }

    /// Issue #34 follow-up: returns None for any of the per-element
    /// pool outputs when the underlying pool is zero.
    #[test]
    fn total_pool_breakdown_skipped_when_no_pool() {
        let env = Env::default();
        for key in [
            "PhysicalTotalHitPool",
            "FireTotalHitPool",
            "ColdTotalHitPool",
            "LightningTotalHitPool",
            "ChaosTotalHitPool",
            "PhysicalTotalPool",
            "FireTotalPool",
            "ColdTotalPool",
            "LightningTotalPool",
            "ChaosTotalPool",
        ] {
            assert!(
                derive_for(&env, key).is_none(),
                "expected None for {key} when pool is zero",
            );
        }
    }

    /// Issue #34 follow-up: FireDotEHP walks Pool → Taken multi
    /// (= 1 - resist) → Final. PoB derives all five `<Element>DotEHP`
    /// outputs as `pool / (1 - resist)`, with no block /
    /// suppression layered (DoT damage doesn't go through hit-time
    /// defences). Worked example: 1100 pool, 75% fire resist →
    /// pool / 0.25 = 4400 fire dot EHP.
    #[test]
    fn fire_dot_ehp_breakdown_walks_pool_and_resist() {
        let mut env = Env::default();
        env.output.set("Life", 1100.0);
        env.output.set("EnergyShield", 0.0);
        env.output.set("Ward", 0.0);
        env.output.set("FireResistTotal", 75.0);
        env.output.set("FireDotEHP", 4400.0);
        let bd = derive_for(&env, "FireDotEHP").unwrap();
        let labels: Vec<&str> = bd.steps.iter().map(|s| s.label.as_str()).collect();
        assert!(labels.contains(&"Pool"));
        assert!(labels.contains(&"Taken multiplier"));
        assert!(labels.contains(&"Fire DoT EHP"));

        let pool = bd.steps.iter().find(|s| s.label == "Pool").unwrap();
        assert!((pool.value.unwrap() - 1100.0).abs() < 1e-9);

        let taken = bd
            .steps
            .iter()
            .find(|s| s.label == "Taken multiplier")
            .unwrap();
        // 1 - 0.75 = 0.25.
        assert!((taken.value.unwrap() - 0.25).abs() < 1e-6);

        assert!((bd.total - 4400.0).abs() < 1e-6);
    }

    /// Issue #34 follow-up: PhysicalDotEHP uses
    /// PhysicalDamageReduction instead of element resist. Worked
    /// example: 1100 pool, 50% phys reduction → pool / 0.50 =
    /// 2200.
    #[test]
    fn physical_dot_ehp_breakdown_walks_pool_and_reduction() {
        let mut env = Env::default();
        env.output.set("Life", 1100.0);
        env.output.set("EnergyShield", 0.0);
        env.output.set("Ward", 0.0);
        env.output.set("PhysicalDamageReduction", 50.0);
        env.output.set("PhysicalDotEHP", 2200.0);
        let bd = derive_for(&env, "PhysicalDotEHP").unwrap();
        let labels: Vec<&str> = bd.steps.iter().map(|s| s.label.as_str()).collect();
        assert!(labels.contains(&"Pool"));
        assert!(labels.contains(&"Taken multiplier"));
        assert!(labels.contains(&"Physical DoT EHP"));
        assert!((bd.total - 2200.0).abs() < 1e-6);
    }

    /// Issue #34 follow-up: returns None for any of the five DoT
    /// EHP outputs when the pool is zero.
    #[test]
    fn dot_ehp_breakdown_skipped_when_no_pool() {
        let env = Env::default();
        for key in [
            "PhysicalDotEHP",
            "FireDotEHP",
            "ColdDotEHP",
            "LightningDotEHP",
            "ChaosDotEHP",
        ] {
            assert!(
                derive_for(&env, key).is_none(),
                "expected None for {key} when pool is zero",
            );
        }
    }

    /// Issue #34 follow-up: PhysicalMaximumHitTaken walks
    /// PhysicalEHP → Pool × 10 cap → min. PoB's defence panel uses
    /// the cap to prevent unrealistic numbers when an element has
    /// high effective resistance / suppression layered. Worked
    /// example: PhysicalEHP 1100 capped at 11000 (1100 pool × 10) →
    /// 1100.
    #[test]
    fn physical_max_hit_taken_breakdown_walks_ehp_and_cap() {
        let mut env = Env::default();
        env.output.set("PhysicalEHP", 1100.0);
        env.output.set("Life", 1100.0);
        env.output.set("EnergyShield", 0.0);
        env.output.set("Ward", 0.0);
        env.output.set("PhysicalMaximumHitTaken", 1100.0);
        let bd = derive_for(&env, "PhysicalMaximumHitTaken").unwrap();
        let labels: Vec<&str> = bd.steps.iter().map(|s| s.label.as_str()).collect();
        assert!(labels.contains(&"Effective HP"));
        assert!(labels.contains(&"Cap"));
        assert!(labels.contains(&"Physical maximum hit taken"));

        let cap = bd.steps.iter().find(|s| s.label == "Cap").unwrap();
        // 1100 pool × 10 = 11000.
        assert!((cap.value.unwrap() - 11000.0).abs() < 1e-6);
        assert!((bd.total - 1100.0).abs() < 1e-9);
    }

    /// Issue #34 follow-up: when EHP exceeds pool × 10 the cap
    /// activates and the final step's explain calls it out.
    #[test]
    fn fire_max_hit_taken_breakdown_flags_capped_case() {
        let mut env = Env::default();
        // Fully fire-resistant build: pool 1000, FireEHP would be 50000
        // (after Purity etc.), capped at pool × 10 = 10000.
        env.output.set("FireEHP", 50000.0);
        env.output.set("Life", 1000.0);
        env.output.set("EnergyShield", 0.0);
        env.output.set("Ward", 0.0);
        env.output.set("FireMaximumHitTaken", 10000.0);
        let bd = derive_for(&env, "FireMaximumHitTaken").unwrap();
        let final_step = bd.steps.last().unwrap();
        assert!(
            final_step
                .explain
                .as_deref()
                .is_some_and(|e| e.contains("capped")),
            "expected 'capped' in explain when EHP > pool × 10, got {:?}",
            final_step.explain
        );
    }

    /// Issue #34 follow-up: returns None for any per-element max-hit
    /// when the pool is zero (no character loaded yet).
    #[test]
    fn maximum_hit_taken_breakdown_skipped_when_no_pool() {
        let env = Env::default();
        for key in [
            "PhysicalMaximumHitTaken",
            "FireMaximumHitTaken",
            "ColdMaximumHitTaken",
            "LightningMaximumHitTaken",
            "ChaosMaximumHitTaken",
        ] {
            assert!(
                derive_for(&env, key).is_none(),
                "expected None for {key} when pool is zero",
            );
        }
    }

    /// Issue #34 follow-up: BlockChanceMax shares the same
    /// default-plus-BASE chain as the max-resist outputs, just with
    /// a different mod-source hint (Glancing Blows / Bone Offering /
    /// ascendancy). Worked example: default 75 + 5 from Glancing
    /// Blows-style node = 80% max block.
    #[test]
    fn block_chance_max_breakdown_walks_default_and_mods() {
        let mut env = Env::default();
        env.mod_db
            .add(Mod::base("BlockChanceMax", 5.0).with_source(Source::Tree));
        env.output.set("BlockChanceMax", 80.0);
        let bd = derive_for(&env, "BlockChanceMax").unwrap();
        let labels: Vec<&str> = bd.steps.iter().map(|s| s.label.as_str()).collect();
        assert!(labels.contains(&"Default"));
        assert!(labels.contains(&"BASE mods"));
        assert!(labels.contains(&"Block chance maximum"));

        let default = bd.steps.iter().find(|s| s.label == "Default").unwrap();
        assert!((default.value.unwrap() - 75.0).abs() < 1e-9);

        let base = bd.steps.iter().find(|s| s.label == "BASE mods").unwrap();
        assert!(
            base.explain
                .as_deref()
                .is_some_and(|e| e.contains("Glancing Blows")),
            "expected Glancing Blows hint in explain, got {:?}",
            base.explain
        );

        assert!((bd.total - 80.0).abs() < 1e-9);
    }

    /// Issue #34 follow-up: at-default (no BASE mods) BlockChanceMax
    /// renders Default + Final only.
    #[test]
    fn block_chance_max_breakdown_collapses_when_default() {
        let mut env = Env::default();
        env.output.set("BlockChanceMax", 75.0);
        let bd = derive_for(&env, "BlockChanceMax").unwrap();
        let labels: Vec<&str> = bd.steps.iter().map(|s| s.label.as_str()).collect();
        assert!(labels.contains(&"Default"));
        assert!(!labels.contains(&"BASE mods"));
        assert!((bd.total - 75.0).abs() < 1e-9);
    }

    /// Issue #34 follow-up: BlockChanceMax returns None when zero
    /// (no character loaded yet).
    #[test]
    fn block_chance_max_breakdown_skipped_when_zero() {
        let env = Env::default();
        assert!(
            derive_for(&env, "BlockChanceMax").is_none(),
            "expected None when BlockChanceMax is zero",
        );
    }

    /// Issue #34 follow-up: FireResistMax walks Default (75 PoE
    /// constant) + BASE mods → total. PoB derives all four
    /// max-resist outputs (Fire / Cold / Lightning / Chaos) the same
    /// way: `75 + Σ BASE("<Element>ResistMax")`. Worked example:
    /// default 75 + 5 from a Loreweave node = 80% max fire resist.
    #[test]
    fn fire_resist_max_breakdown_walks_default_and_mods() {
        let mut env = Env::default();
        env.mod_db
            .add(Mod::base("FireResistMax", 5.0).with_source(Source::Tree));
        env.output.set("FireResistMax", 80.0);
        let bd = derive_for(&env, "FireResistMax").unwrap();
        let labels: Vec<&str> = bd.steps.iter().map(|s| s.label.as_str()).collect();
        assert!(labels.contains(&"Default"));
        assert!(labels.contains(&"BASE mods"));
        assert!(labels.contains(&"Fire resistance maximum"));
        let default = bd.steps.iter().find(|s| s.label == "Default").unwrap();
        assert!((default.value.unwrap() - 75.0).abs() < 1e-9);
        assert!((bd.total - 80.0).abs() < 1e-9);
    }

    /// Issue #34 follow-up: ColdResistMax / LightningResistMax /
    /// ChaosResistMax all share the same default + BASE-mod chain
    /// as FireResistMax. Worked example: default 75 with no mods →
    /// 75% on each.
    #[test]
    fn other_resist_max_breakdowns_walk_same_chain() {
        for key in ["ColdResistMax", "LightningResistMax", "ChaosResistMax"] {
            let mut env = Env::default();
            env.output.set(key, 75.0);
            let bd = derive_for(&env, key).unwrap_or_else(|| panic!("{key}: dispatch missing"));
            let labels: Vec<&str> = bd.steps.iter().map(|s| s.label.as_str()).collect();
            assert!(labels.contains(&"Default"), "{key}: missing Default");
            assert!(
                !labels.contains(&"BASE mods"),
                "{key}: BASE step should be suppressed at default"
            );
            assert!((bd.total - 75.0).abs() < 1e-9, "{key}: wrong total");
        }
    }

    /// Issue #34 follow-up: returns None when the max resist is zero
    /// (no character loaded yet).
    #[test]
    fn resist_max_breakdown_skipped_when_zero() {
        let env = Env::default();
        for key in [
            "FireResistMax",
            "ColdResistMax",
            "LightningResistMax",
            "ChaosResistMax",
        ] {
            assert!(
                derive_for(&env, key).is_none(),
                "expected None for {key} when zero",
            );
        }
    }

    /// Issue #34 follow-up: ManaRegenRecovery is just an alias for
    /// ManaRegen — `perform_basic_stats` sets it to the same value
    /// so PoB's flask-recovery code can read it under that name.
    /// Surfacing the alias in the breakdown panel saves users from
    /// hunting for "why is this the same number as ManaRegen?".
    #[test]
    fn mana_regen_recovery_breakdown_aliases_mana_regen() {
        let mut env = Env::default();
        env.output.set("ManaRegen", 12.5);
        env.output.set("ManaRegenRecovery", 12.5);
        let bd = derive_for(&env, "ManaRegenRecovery").unwrap();
        let labels: Vec<&str> = bd.steps.iter().map(|s| s.label.as_str()).collect();
        assert!(labels.contains(&"Mana regen"));
        assert!(labels.contains(&"Mana regen recovery"));

        let alias = bd.steps.iter().find(|s| s.label == "Mana regen").unwrap();
        assert!((alias.value.unwrap() - 12.5).abs() < 1e-9);
        assert!(
            alias
                .explain
                .as_deref()
                .is_some_and(|e| e.contains("ManaRegen") || e.contains("alias")),
            "expected alias hint in explain, got {:?}",
            alias.explain
        );
        assert!((bd.total - 12.5).abs() < 1e-9);
    }

    /// Issue #34 follow-up: returns None when ManaRegen is zero
    /// (no character loaded yet).
    #[test]
    fn mana_regen_recovery_breakdown_skipped_when_zero() {
        let env = Env::default();
        assert!(
            derive_for(&env, "ManaRegenRecovery").is_none(),
            "expected None when ManaRegenRecovery is zero",
        );
    }

    /// Issue #34 follow-up: AccuracyHitChance is just an alias for
    /// MainSkillHitChance — `perform_skill_dps` sets it to the same
    /// value so PoB's character-level code can read the active skill's
    /// hit chance under attack-skill terminology. Surfacing the alias
    /// in the breakdown panel saves users from hunting for "why is
    /// this the same number as MainSkillHitChance?".
    #[test]
    fn accuracy_hit_chance_breakdown_aliases_main_skill_hit_chance() {
        let mut env = Env::default();
        env.output.set("MainSkillHitChance", 95.0);
        env.output.set("AccuracyHitChance", 95.0);
        let bd = derive_for(&env, "AccuracyHitChance").unwrap();
        let labels: Vec<&str> = bd.steps.iter().map(|s| s.label.as_str()).collect();
        assert!(
            labels.iter().any(|l| l.contains("Main skill hit chance")),
            "expected back-link to Main skill hit chance, got {labels:?}",
        );
        let alias = bd
            .steps
            .iter()
            .find(|s| s.label.contains("Main skill hit chance"))
            .unwrap();
        assert!((alias.value.unwrap() - 95.0).abs() < 1e-9);
        assert!(
            alias
                .explain
                .as_deref()
                .is_some_and(|e| e.contains("MainSkillHitChance") || e.contains("alias")),
            "expected alias hint in explain, got {:?}",
            alias.explain
        );
        assert!((bd.total - 95.0).abs() < 1e-9);
    }

    /// Issue #34 follow-up: returns None when no skill is bound
    /// (AccuracyHitChance defaults to zero on an empty env).
    #[test]
    fn accuracy_hit_chance_breakdown_skipped_when_no_skill() {
        let env = Env::default();
        assert!(
            derive_for(&env, "AccuracyHitChance").is_none(),
            "expected None when AccuracyHitChance is zero",
        );
    }

    /// Issue #34 follow-up: ShockChance walks the same on-hit +
    /// on-crit composition as IgniteChance, sourced from
    /// ShockChanceOnHit. Worked example: 40% on-hit + 25% crit →
    /// 40 × 0.75 + 100 × 0.25 = 55%.
    #[test]
    fn shock_chance_breakdown_walks_on_hit_and_crit() {
        let mut env = Env::default();
        env.output.set("ShockChanceOnHit", 40.0);
        env.output.set("MainSkillCritChance", 25.0);
        env.output.set("ShockChance", 55.0);
        let bd = derive_for(&env, "ShockChance").unwrap();
        let labels: Vec<&str> = bd.steps.iter().map(|s| s.label.as_str()).collect();
        assert!(labels.contains(&"On-hit chance"));
        assert!(labels.contains(&"On-crit chance"));
        assert!(labels.contains(&"Crit chance"));
        assert!(labels.contains(&"Shock chance"));

        let final_step = bd.steps.last().unwrap();
        assert!(
            final_step
                .explain
                .as_deref()
                .is_some_and(|e| e.contains("shock") || e.contains("100%")),
            "expected shock formula in explain, got {:?}",
            final_step.explain
        );
        assert!((bd.total - 55.0).abs() < 1e-9);
    }

    /// Issue #34 follow-up: FreezeChance walks the same shape with
    /// FreezeChanceOnHit. Worked example: 0% on-hit + 25% crit →
    /// 25%.
    #[test]
    fn freeze_chance_breakdown_walks_on_hit_and_crit() {
        let mut env = Env::default();
        env.output.set("FreezeChanceOnHit", 0.0);
        env.output.set("MainSkillCritChance", 25.0);
        env.output.set("FreezeChance", 25.0);
        let bd = derive_for(&env, "FreezeChance").unwrap();
        let labels: Vec<&str> = bd.steps.iter().map(|s| s.label.as_str()).collect();
        assert!(labels.contains(&"Freeze chance"));
        assert!((bd.total - 25.0).abs() < 1e-9);

        let on_crit = bd
            .steps
            .iter()
            .find(|s| s.label == "On-crit chance")
            .unwrap();
        assert!(
            on_crit
                .explain
                .as_deref()
                .is_some_and(|e| e.contains("freeze")),
            "expected freeze in on-crit explain, got {:?}",
            on_crit.explain
        );
    }

    /// Issue #34 follow-up: both Shock and Freeze return None when
    /// their final chance is zero.
    #[test]
    fn shock_freeze_chance_breakdowns_skipped_when_zero() {
        let env = Env::default();
        assert!(
            derive_for(&env, "ShockChance").is_none(),
            "expected None for ShockChance when zero",
        );
        assert!(
            derive_for(&env, "FreezeChance").is_none(),
            "expected None for FreezeChance when zero",
        );
    }

    /// Issue #34 follow-up: IgniteChance walks the on-hit + on-crit
    /// composition. PoB derives it as
    /// `IgniteChanceOnHit × (1 - crit) + 100 × crit`, since crits
    /// always ignite (PoE rule). The Calcs tab surfaced just the
    /// final percentage; users couldn't see how on-hit chance and
    /// crit chance composed.
    ///
    /// Worked example: 0% on-hit + 25% crit → 0 × 0.75 + 100 × 0.25
    /// = 25% effective ignite chance.
    #[test]
    fn ignite_chance_breakdown_walks_on_hit_and_crit() {
        let mut env = Env::default();
        env.output.set("IgniteChanceOnHit", 0.0);
        env.output.set("MainSkillCritChance", 25.0);
        env.output.set("IgniteChance", 25.0);
        let bd = derive_for(&env, "IgniteChance").unwrap();
        let labels: Vec<&str> = bd.steps.iter().map(|s| s.label.as_str()).collect();
        assert!(labels.contains(&"On-hit chance"));
        assert!(labels.contains(&"On-crit chance"));
        assert!(labels.contains(&"Crit chance"));
        assert!(labels.contains(&"Ignite chance"));

        let on_hit = bd
            .steps
            .iter()
            .find(|s| s.label == "On-hit chance")
            .unwrap();
        assert!((on_hit.value.unwrap() - 0.0).abs() < 1e-9);

        let on_crit = bd
            .steps
            .iter()
            .find(|s| s.label == "On-crit chance")
            .unwrap();
        assert!((on_crit.value.unwrap() - 100.0).abs() < 1e-9);

        assert!((bd.total - 25.0).abs() < 1e-9);
    }

    /// Issue #34 follow-up: with both on-hit and crit contributing
    /// the chain composes additively. Worked example: 30% on-hit +
    /// 50% crit → 30 × 0.5 + 100 × 0.5 = 15 + 50 = 65%.
    #[test]
    fn ignite_chance_breakdown_combines_on_hit_and_crit() {
        let mut env = Env::default();
        env.output.set("IgniteChanceOnHit", 30.0);
        env.output.set("MainSkillCritChance", 50.0);
        env.output.set("IgniteChance", 65.0);
        let bd = derive_for(&env, "IgniteChance").unwrap();
        let final_step = bd.steps.last().unwrap();
        assert_eq!(final_step.label, "Ignite chance");
        assert!((final_step.value.unwrap() - 65.0).abs() < 1e-9);
    }

    /// Issue #34 follow-up: returns None when ignite chance is zero
    /// (no on-hit ignite mods + 0% crit chance).
    #[test]
    fn ignite_chance_breakdown_skipped_when_zero() {
        let env = Env::default();
        assert!(
            derive_for(&env, "IgniteChance").is_none(),
            "expected None when IgniteChance is zero",
        );
    }

    /// Issue #34 follow-up: BleedChance walks BASE-mod sum →
    /// clamped final percentage. PoB stores `BleedChance` as a
    /// percentage (0–100) derived from `clamp(0, 100, Σ BASE)`. The
    /// breakdown surfaces the source mods so users can see what's
    /// driving their bleed chance. Worked example: 25 from gem +
    /// 35 from passive = 60% bleed chance.
    #[test]
    fn bleed_chance_breakdown_walks_base_mods() {
        let mut env = Env::default();
        env.mod_db
            .add(Mod::base("BleedChance", 25.0).with_source(Source::Tree));
        env.mod_db
            .add(Mod::base("BleedChance", 35.0).with_source(Source::Item(2)));
        env.output.set("BleedChance", 60.0);
        let bd = derive_for(&env, "BleedChance").unwrap();
        let labels: Vec<&str> = bd.steps.iter().map(|s| s.label.as_str()).collect();
        assert!(labels.contains(&"BASE mods"));
        assert!(labels.contains(&"Bleed chance"));

        let base = bd.steps.iter().find(|s| s.label == "BASE mods").unwrap();
        assert!((base.value.unwrap() - 60.0).abs() < 1e-9);
        assert!((bd.total - 60.0).abs() < 1e-9);
    }

    /// Issue #34 follow-up: BleedChance returns None when zero —
    /// no mods, no breakdown to display.
    #[test]
    fn bleed_chance_breakdown_skipped_when_zero() {
        let env = Env::default();
        assert!(
            derive_for(&env, "BleedChance").is_none(),
            "expected None when BleedChance is zero",
        );
    }

    /// Issue #34 follow-up: BleedChance saturates at 100% — final
    /// step's explain mentions the cap when the BASE sum exceeds
    /// 100. Worked example: 80 from gem + 60 from gear → 140 BASE,
    /// clamped to 100%.
    #[test]
    fn bleed_chance_breakdown_flags_capped_case() {
        let mut env = Env::default();
        env.mod_db
            .add(Mod::base("BleedChance", 80.0).with_source(Source::Skill("Reaper".into())));
        env.mod_db
            .add(Mod::base("BleedChance", 60.0).with_source(Source::Item(2)));
        env.output.set("BleedChance", 100.0);
        let bd = derive_for(&env, "BleedChance").unwrap();
        let final_step = bd.steps.last().unwrap();
        assert_eq!(final_step.label, "Bleed chance");
        assert!(
            final_step
                .explain
                .as_deref()
                .is_some_and(|e| e.contains("capped")),
            "expected 'capped' in explain when BASE sum > 100, got {:?}",
            final_step.explain
        );
    }

    /// Issue #34 follow-up: PoisonStackLimit walks default + BASE mods.
    /// PoB derives the limit as `max(50 default, Σ BASE(PoisonStackLimit))`
    /// — so uniques like Volkuur's that explicitly raise the cap stack
    /// on top of the default 50, while builds without such mods land
    /// at exactly 50. The breakdown surfaces the chain so users can
    /// see whether their cap is the default or is being lifted.
    /// Worked example: default 50 + 100 from Volkuur's = 150 limit.
    #[test]
    fn poison_stack_limit_breakdown_walks_default_and_mods() {
        let mut env = Env::default();
        env.mod_db
            .add(Mod::base("PoisonStackLimit", 100.0).with_source(Source::Item(2)));
        env.output.set("PoisonStackLimit", 150.0);
        let bd = derive_for(&env, "PoisonStackLimit").unwrap();
        let labels: Vec<&str> = bd.steps.iter().map(|s| s.label.as_str()).collect();
        assert!(labels.contains(&"Default"));
        assert!(labels.contains(&"BASE mods"));
        assert!(labels.contains(&"Poison stack limit"));

        let default = bd.steps.iter().find(|s| s.label == "Default").unwrap();
        assert!((default.value.unwrap() - 50.0).abs() < 1e-9);
        assert!((bd.total - 150.0).abs() < 1e-9);
    }

    /// Issue #34 follow-up: with no PoisonStackLimit BASE mods the
    /// breakdown collapses to Default + Final at exactly 50.
    #[test]
    fn poison_stack_limit_breakdown_collapses_when_default() {
        let mut env = Env::default();
        env.output.set("PoisonStackLimit", 50.0);
        let bd = derive_for(&env, "PoisonStackLimit").unwrap();
        let labels: Vec<&str> = bd.steps.iter().map(|s| s.label.as_str()).collect();
        assert!(labels.contains(&"Default"));
        assert!(!labels.contains(&"BASE mods"));
        assert!(labels.contains(&"Poison stack limit"));
        assert!((bd.total - 50.0).abs() < 1e-9);
    }

    /// Issue #34 follow-up: PoisonStacks walks Cast/attack rate ×
    /// Poison duration → nominal capacity, capped at the stack
    /// limit. PoB computes steady-state stack count as
    /// `min(speed × duration × chance, PoisonStackLimit)` where
    /// chance is back-derived from `stacks / (speed × duration)`
    /// when the cap isn't active. The breakdown surfaces the
    /// composition so users can see whether they're cap-limited or
    /// rate-limited.
    ///
    /// Worked example: 5 cps × 3s × 100% = 15 stacks (un-capped).
    #[test]
    fn poison_stacks_breakdown_walks_speed_duration_chance() {
        let mut env = Env::default();
        env.output.set("MainSkillSpeed", 5.0);
        env.output.set("PoisonDuration", 3.0);
        env.output.set("PoisonStacks", 15.0);
        env.output.set("PoisonStackLimit", 50.0);
        let bd = derive_for(&env, "PoisonStacks").unwrap();
        let labels: Vec<&str> = bd.steps.iter().map(|s| s.label.as_str()).collect();
        assert!(labels.contains(&"Skill speed"));
        assert!(labels.contains(&"Poison duration"));
        assert!(labels.contains(&"Effective chance"));
        assert!(labels.contains(&"Stack limit"));
        assert!(labels.contains(&"Poison stacks"));

        let chance = bd
            .steps
            .iter()
            .find(|s| s.label == "Effective chance")
            .unwrap();
        // 15 / (5 × 3) = 1.0 = 100%.
        assert!((chance.value.unwrap() - 1.0).abs() < 1e-6);
        assert!((bd.total - 15.0).abs() < 1e-9);
    }

    /// Issue #34 follow-up: when the steady-state stack count would
    /// exceed PoisonStackLimit, the final step's explain calls out
    /// the cap. Worked example: 5 cps × 5s × 200% chance would yield
    /// 50 stacks but the default stack limit is 50, so the result
    /// is exactly the cap and the explain notes it.
    #[test]
    fn poison_stacks_breakdown_flags_capped_case() {
        let mut env = Env::default();
        env.output.set("MainSkillSpeed", 5.0);
        env.output.set("PoisonDuration", 5.0);
        env.output.set("PoisonStacks", 50.0);
        env.output.set("PoisonStackLimit", 50.0);
        let bd = derive_for(&env, "PoisonStacks").unwrap();
        let final_step = bd.steps.last().unwrap();
        assert_eq!(final_step.label, "Poison stacks");
        assert!(
            final_step
                .explain
                .as_deref()
                .is_some_and(|e| e.contains("capped")),
            "expected 'capped' in explain when stacks == limit, got {:?}",
            final_step.explain
        );
    }

    /// Issue #34 follow-up: returns None when the build doesn't
    /// inflict poison (stacks == 0).
    #[test]
    fn poison_stacks_breakdown_skipped_when_no_poison() {
        let env = Env::default();
        assert!(
            derive_for(&env, "PoisonStacks").is_none(),
            "expected None when PoisonStacks is zero",
        );
    }

    /// Issue #34 follow-up: PoisonDuration walks the same Base ×
    /// (1 + INC%) chain as IgniteDuration but with a 2-second base.
    /// Worked example: base 2 × (1 + 50%) = 3s.
    #[test]
    fn poison_duration_breakdown_walks_base_and_inc() {
        let mut env = Env::default();
        env.mod_db
            .add(Mod::inc("PoisonDuration", 50.0).with_source(Source::Tree));
        env.output.set("PoisonDuration", 3.0);
        let bd = derive_for(&env, "PoisonDuration").unwrap();
        let labels: Vec<&str> = bd.steps.iter().map(|s| s.label.as_str()).collect();
        assert!(labels.contains(&"Base"));
        assert!(labels.contains(&"Increased"));
        assert!(labels.contains(&"Poison duration"));

        let base = bd.steps.iter().find(|s| s.label == "Base").unwrap();
        assert!((base.value.unwrap() - 2.0).abs() < 1e-9);
        assert!((bd.total - 3.0).abs() < 1e-6);
    }

    /// Issue #34 follow-up: BleedDuration walks the same Base ×
    /// (1 + INC%) chain with a 5-second base. Worked example: base
    /// 5 × (1 + 30%) = 6.5s.
    #[test]
    fn bleed_duration_breakdown_walks_base_and_inc() {
        let mut env = Env::default();
        env.mod_db
            .add(Mod::inc("BleedDuration", 30.0).with_source(Source::Tree));
        env.output.set("BleedDuration", 6.5);
        let bd = derive_for(&env, "BleedDuration").unwrap();
        let labels: Vec<&str> = bd.steps.iter().map(|s| s.label.as_str()).collect();
        assert!(labels.contains(&"Base"));
        assert!(labels.contains(&"Increased"));
        assert!(labels.contains(&"Bleed duration"));

        let base = bd.steps.iter().find(|s| s.label == "Base").unwrap();
        assert!((base.value.unwrap() - 5.0).abs() < 1e-9);
        assert!((bd.total - 6.5).abs() < 1e-6);
    }

    /// Issue #34 follow-up: PoisonDuration / BleedDuration both
    /// return None when the duration is zero.
    #[test]
    fn poison_bleed_duration_breakdowns_skipped_when_zero() {
        let env = Env::default();
        assert!(
            derive_for(&env, "PoisonDuration").is_none(),
            "expected None for PoisonDuration when zero",
        );
        assert!(
            derive_for(&env, "BleedDuration").is_none(),
            "expected None for BleedDuration when zero",
        );
    }

    /// Issue #34 follow-up: IgniteDuration breakdown. PoB derives it
    /// as `4.0 × (1 + Σ INC(IgniteDuration) / 100)` (with the 4-second
    /// base coming from the upstream ignite-duration constant). The
    /// Calcs tab surfaced just the seconds value; users couldn't see
    /// where the inc came from. Worked example: base 4 × (1 + 50%) =
    /// 6s.
    #[test]
    fn ignite_duration_breakdown_walks_base_and_inc() {
        let mut env = Env::default();
        env.mod_db
            .add(Mod::inc("IgniteDuration", 50.0).with_source(Source::Tree));
        env.output.set("IgniteDuration", 6.0);
        let bd = derive_for(&env, "IgniteDuration").unwrap();
        let labels: Vec<&str> = bd.steps.iter().map(|s| s.label.as_str()).collect();
        assert!(labels.contains(&"Base"));
        assert!(labels.contains(&"Increased"));
        assert!(labels.contains(&"Ignite duration"));

        let base = bd.steps.iter().find(|s| s.label == "Base").unwrap();
        // 4s PoE constant.
        assert!((base.value.unwrap() - 4.0).abs() < 1e-9);
        assert!((bd.total - 6.0).abs() < 1e-6);
    }

    /// Issue #34 follow-up: with no Inc IgniteDuration mods the
    /// breakdown collapses to Base + Final (no Inc step).
    #[test]
    fn ignite_duration_breakdown_collapses_when_no_inc() {
        let mut env = Env::default();
        env.output.set("IgniteDuration", 4.0);
        let bd = derive_for(&env, "IgniteDuration").unwrap();
        let labels: Vec<&str> = bd.steps.iter().map(|s| s.label.as_str()).collect();
        assert!(labels.contains(&"Base"));
        assert!(!labels.contains(&"Increased"));
        assert!(labels.contains(&"Ignite duration"));
        assert!((bd.total - 4.0).abs() < 1e-9);
    }

    /// Issue #34 follow-up: returns None when ignite duration is
    /// zero (not yet computed).
    #[test]
    fn ignite_duration_breakdown_skipped_when_zero() {
        let env = Env::default();
        assert!(
            derive_for(&env, "IgniteDuration").is_none(),
            "expected None when IgniteDuration is zero",
        );
    }

    /// Issue #34 follow-up: MainSkillShockMult breakdown. PoB
    /// applies shock as `EnemyDamageTaken INC` weighted by an
    /// effective shock chance:
    ///
    ///   chance = CurseShockChanceOnHit × (1 - crit) + 100 × crit
    ///   mult   = 1 + chance / 100
    ///
    /// Worked example: 60% curse-on-hit shock + 25% crit chance →
    /// 60 × 0.75 + 100 × 0.25 = 70% effective → 1.70× damage.
    #[test]
    fn shock_mult_breakdown_walks_curse_crit_to_chance() {
        let mut env = Env::default();
        env.output.set("CurseShockChanceOnHit", 60.0);
        env.output.set("MainSkillCritChance", 25.0);
        env.output.set("MainSkillShockMult", 1.70);
        let bd = derive_for(&env, "MainSkillShockMult").unwrap();
        let labels: Vec<&str> = bd.steps.iter().map(|s| s.label.as_str()).collect();
        assert!(labels.contains(&"Curse-on-hit chance"));
        assert!(labels.contains(&"Crit chance"));
        assert!(labels.contains(&"Effective shock chance"));
        assert!(labels.contains(&"Shock multiplier"));

        let chance = bd
            .steps
            .iter()
            .find(|s| s.label == "Effective shock chance")
            .unwrap();
        // 60 × 0.75 + 100 × 0.25 = 70.
        assert!(
            (chance.value.unwrap() - 70.0).abs() < 1e-6,
            "expected 70% effective chance, got {:?}",
            chance.value
        );
        assert!((bd.total - 1.70).abs() < 1e-6);
    }

    /// Issue #34 follow-up: with no curse-on-hit shock chance the
    /// breakdown collapses to a single "no shock active" line at
    /// 1.0×. PoB's gate (only emit non-1.0 mult when curse is
    /// active) is mirrored exactly.
    #[test]
    fn shock_mult_breakdown_collapses_when_no_curse() {
        let mut env = Env::default();
        env.output.set("MainSkillShockMult", 1.0);
        let bd = derive_for(&env, "MainSkillShockMult").unwrap();
        let labels: Vec<&str> = bd.steps.iter().map(|s| s.label.as_str()).collect();
        assert!(labels.contains(&"Shock multiplier"));
        // Without curse-shock-on-hit there's no chance composition
        // to walk, so the intermediate steps are suppressed.
        assert!(!labels.contains(&"Curse-on-hit chance"));
        assert!(!labels.contains(&"Effective shock chance"));
        let final_step = bd.steps.last().unwrap();
        assert!((final_step.value.unwrap() - 1.0).abs() < 1e-9);
    }

    /// Issue #34 follow-up: ProjectileMultiplier walks the
    /// Config "projectiles hitting target" → projectile cap → final
    /// damage multiplier. PoB caps the user's pick into
    /// `[1, ProjectileCount]` and uses the result as a per-hit damage
    /// multiplier (focal-point Tornado Shot, point-blank Barrage,
    /// etc.). Worked example: 3 hits requested + 5 projectiles → 3
    /// (un-capped, multiplier 3×).
    #[test]
    fn projectile_multiplier_breakdown_walks_config_and_cap() {
        let mut env = Env::default();
        env.output.set("ProjectileCount", 5.0);
        env.output.set("ProjectileMultiplier", 3.0);
        let bd = derive_for(&env, "ProjectileMultiplier").unwrap();
        let labels: Vec<&str> = bd.steps.iter().map(|s| s.label.as_str()).collect();
        assert!(labels.contains(&"Hits requested"));
        assert!(labels.contains(&"Cap"));
        assert!(labels.contains(&"Projectile multiplier"));

        let cap = bd.steps.iter().find(|s| s.label == "Cap").unwrap();
        assert!((cap.value.unwrap() - 5.0).abs() < 1e-9);
        assert!((bd.total - 3.0).abs() < 1e-9);
    }

    /// Issue #34 follow-up: when ProjectileMultiplier == ProjectileCount
    /// the chain implies the user requested at least the projectile
    /// count and got capped. Final step's explain mentions the cap.
    #[test]
    fn projectile_multiplier_breakdown_flags_capped_case() {
        let mut env = Env::default();
        env.output.set("ProjectileCount", 5.0);
        env.output.set("ProjectileMultiplier", 5.0);
        let bd = derive_for(&env, "ProjectileMultiplier").unwrap();
        let final_step = bd.steps.last().unwrap();
        assert_eq!(final_step.label, "Projectile multiplier");
        assert!(
            final_step
                .explain
                .as_deref()
                .is_some_and(|e| e.contains("capped")),
            "expected 'capped' in explain when multiplier == count, got {:?}",
            final_step.explain
        );
    }

    /// Issue #34 follow-up: returns None when not projectile-based.
    #[test]
    fn projectile_multiplier_breakdown_skipped_when_no_projectiles() {
        let env = Env::default();
        assert!(
            derive_for(&env, "ProjectileMultiplier").is_none(),
            "expected None when ProjectileCount is zero",
        );
    }

    /// total projectile count as `1 (primary) + additional
    /// projectiles`, where the additional count comes from the
    /// skill's `number_of_additional_projectiles` stat plus tree /
    /// gear mods aggregated into the EvalState. The Calcs tab
    /// surfaced the total as a single number; users couldn't see
    /// where the additional count came from.
    ///
    /// Worked example: 5 total → 1 primary + 4 additional.
    #[test]
    fn projectile_count_breakdown_walks_primary_plus_additional() {
        let mut env = Env::default();
        env.output.set("ProjectileCount", 5.0);
        let bd = derive_for(&env, "ProjectileCount").unwrap();
        let labels: Vec<&str> = bd.steps.iter().map(|s| s.label.as_str()).collect();
        assert!(labels.contains(&"Primary"));
        assert!(labels.contains(&"Additional"));
        assert!(labels.contains(&"Projectile count"));

        let primary = bd.steps.iter().find(|s| s.label == "Primary").unwrap();
        assert!((primary.value.unwrap() - 1.0).abs() < 1e-9);

        let additional = bd.steps.iter().find(|s| s.label == "Additional").unwrap();
        assert!((additional.value.unwrap() - 4.0).abs() < 1e-9);

        assert!((bd.total - 5.0).abs() < 1e-9);
    }

    /// Issue #34 follow-up: a single-projectile skill (e.g. Fireball
    /// without LMP) yields the Primary step + Final, but the
    /// Additional step is suppressed since the value is zero.
    #[test]
    fn projectile_count_breakdown_suppresses_additional_when_one() {
        let mut env = Env::default();
        env.output.set("ProjectileCount", 1.0);
        let bd = derive_for(&env, "ProjectileCount").unwrap();
        let labels: Vec<&str> = bd.steps.iter().map(|s| s.label.as_str()).collect();
        assert!(labels.contains(&"Primary"));
        assert!(!labels.contains(&"Additional"));
        assert!(labels.contains(&"Projectile count"));
        assert!((bd.total - 1.0).abs() < 1e-9);
    }

    /// Issue #34 follow-up: returns None when the skill isn't
    /// projectile-based (no ProjectileCount in output).
    #[test]
    fn projectile_count_breakdown_skipped_when_not_projectile() {
        let env = Env::default();
        assert!(
            derive_for(&env, "ProjectileCount").is_none(),
            "expected None when ProjectileCount is zero",
        );
    }

    /// Issue #34 follow-up: AreaOfEffectRadiusMetres breakdown.
    /// PoB exposes the AoE radius in metres alongside the engine
    /// units; conversion is just `radius / 10`. Surfacing the chain
    /// lets users see the engine-units value next to the metres
    /// value without context-switching. Worked example: 26 engine
    /// units → 2.6 metres.
    #[test]
    fn aoe_radius_metres_breakdown_walks_units_to_metres() {
        let mut env = Env::default();
        env.output.set("AreaOfEffectRadius", 26.0);
        env.output.set("AreaOfEffectRadiusMetres", 2.6);
        let bd = derive_for(&env, "AreaOfEffectRadiusMetres").unwrap();
        let labels: Vec<&str> = bd.steps.iter().map(|s| s.label.as_str()).collect();
        assert!(labels.contains(&"Radius (engine units)"));
        assert!(labels.contains(&"Area of effect radius (metres)"));

        let units = bd
            .steps
            .iter()
            .find(|s| s.label == "Radius (engine units)")
            .unwrap();
        assert!((units.value.unwrap() - 26.0).abs() < 1e-9);
        assert!((bd.total - 2.6).abs() < 1e-6);
    }

    /// Issue #34 follow-up: returns None when the engine-unit radius
    /// is zero (non-AoE skill or no skill loaded).
    #[test]
    fn aoe_radius_metres_breakdown_skipped_when_no_radius() {
        let env = Env::default();
        assert!(
            derive_for(&env, "AreaOfEffectRadiusMetres").is_none(),
            "expected None when AoE radius is zero",
        );
    }

    ///   radius = floor(base × floor(100 × sqrt(area_mod)) / 100)
    ///
    /// Surfaced steps walk Base radius → Area-mod multiplier →
    /// Radius scaling (sqrt(mod)) → final radius. The sqrt step is
    /// what makes the relationship between %AoE and circle radius
    /// non-linear (a +50% area mod is only a +22% radius).
    ///
    /// Worked example: base 22, area mod 1.44 → sqrt(1.44) = 1.2 →
    /// floor(22 × 1.20) = floor(26.4) = 26.
    #[test]
    fn aoe_radius_breakdown_walks_base_mod_sqrt_to_final() {
        let mut env = Env::default();
        env.output.set("AoERadius", 22.0);
        env.output.set("AreaOfEffectMod", 1.44);
        env.output.set("AreaOfEffectRadius", 26.0);
        let bd = derive_for(&env, "AreaOfEffectRadius").unwrap();
        let labels: Vec<&str> = bd.steps.iter().map(|s| s.label.as_str()).collect();
        assert!(labels.contains(&"Base radius"));
        assert!(labels.contains(&"Area mod"));
        assert!(labels.contains(&"Radius scaling"));
        assert!(labels.contains(&"Area of effect radius"));

        let base = bd.steps.iter().find(|s| s.label == "Base radius").unwrap();
        assert!((base.value.unwrap() - 22.0).abs() < 1e-9);

        let scale = bd
            .steps
            .iter()
            .find(|s| s.label == "Radius scaling")
            .unwrap();
        // sqrt(1.44) = 1.2.
        assert!((scale.value.unwrap() - 1.2).abs() < 1e-6);

        assert!((bd.total - 26.0).abs() < 1e-6);
    }

    /// Issue #34 follow-up: with base radius 0 the dispatch returns
    /// None — the panel falls back to the generic mods view rather
    /// than rendering a 0-radius row with no context.
    #[test]
    fn aoe_radius_breakdown_skipped_when_no_base() {
        let env = Env::default();
        assert!(
            derive_for(&env, "AreaOfEffectRadius").is_none(),
            "expected None when base AoE radius is zero",
        );
    }

    /// Issue #34 follow-up: AreaOfEffectMod walks through the same
    /// `(1 + Σ INC/100) × Π MORE` shape as the speed mults, just on
    /// the `AreaOfEffect` mod store. Final step is the AoE mod
    /// multiplier.
    #[test]
    fn aoe_mod_breakdown_recovers_inc_and_more() {
        let mut env = Env::default();
        // 30% INC + 20% MORE → 1.3 × 1.2 = 1.56 area mod.
        env.mod_db
            .add(Mod::inc("AreaOfEffect", 30.0).with_source(Source::Tree));
        env.mod_db
            .add(Mod::more("AreaOfEffect", 20.0).with_source(Source::Item(2)));
        env.output.set("AreaOfEffectMod", 1.56);
        let bd = derive_for(&env, "AreaOfEffectMod").unwrap();
        assert!(bd.steps.iter().any(|s| s.label == "Increased"));
        assert!(bd.steps.iter().any(|s| s.label == "More"));
        let final_step = bd.steps.last().unwrap();
        assert_eq!(final_step.label, "Area of effect modifier");
        assert!((final_step.value.unwrap_or(0.0) - 1.56).abs() < 1e-6);
    }

    /// Issue #34 follow-up: with no AoE mods at all the dispatch
    /// still yields a non-empty breakdown — the (empty) Inc / More
    /// steps are skipped but the final 1.0× line still anchors the
    /// view, mirroring the speed-mult helper's behaviour.
    #[test]
    fn aoe_mod_breakdown_with_no_mods_still_shows_final_step() {
        let mut env = Env::default();
        env.output.set("AreaOfEffectMod", 1.0);
        let bd = derive_for(&env, "AreaOfEffectMod").unwrap();
        assert!(!bd.steps.iter().any(|s| s.label == "Increased"));
        assert!(!bd.steps.iter().any(|s| s.label == "More"));
        let final_step = bd.steps.last().unwrap();
        assert_eq!(final_step.label, "Area of effect modifier");
        assert!((final_step.value.unwrap_or(0.0) - 1.0).abs() < 1e-6);
    }

    #[test]
    fn cast_speed_breakdown_recovers_inc_and_more() {
        let env = env_with_output();
        let bd = derive_for(&env, "CastSpeedMult").unwrap();
        // Should show the +10% INC line.
        assert!(bd.steps.iter().any(|s| s.label == "Increased"));
        let cast_step = bd.steps.last().unwrap();
        assert_eq!(cast_step.label, "Cast speed multiplier");
        assert!((cast_step.value.unwrap_or(0.0) - 1.30).abs() < 1e-6);
    }

    #[test]
    fn main_skill_speed_picks_cast_pool_when_cast_mult_is_dominant() {
        let env = env_with_output();
        let bd = derive_for(&env, "MainSkillSpeed").unwrap();
        let final_step = bd.steps.last().unwrap();
        assert_eq!(final_step.label, "Casts per second");
    }

    #[test]
    fn crit_chance_breakdown_includes_inc_step() {
        let env = env_with_output();
        let bd = derive_for(&env, "CritChance").unwrap();
        assert!(bd.steps.iter().any(|s| s.label.starts_with("Skill base")));
        assert!(bd.steps.iter().any(|s| s.label.starts_with("Increased")));
    }

    #[test]
    fn crit_multiplier_breakdown_adds_base_extra() {
        let env = env_with_output();
        let bd = derive_for(&env, "CritMultiplier").unwrap();
        // Base + additional + total = 3 steps.
        assert!(bd.steps.iter().any(|s| s.label == "Base crit multiplier"));
        assert!(bd
            .steps
            .iter()
            .any(|s| s.label == "+ additional crit damage"));
        assert!(bd.steps.iter().any(|s| s.label == "Crit multiplier"));
    }

    #[test]
    fn crit_effect_breakdown_chains_chance_and_multi() {
        let env = env_with_output();
        let bd = derive_for(&env, "CritEffect").unwrap();
        let total = bd.total;
        // CritEffect was set to 1.03 explicitly.
        assert!((total - 1.03).abs() < 1e-6);
    }

    #[test]
    fn movement_speed_breakdown_uses_movement_pool() {
        let mut env = env_with_output();
        env.mod_db
            .add(Mod::inc("MovementSpeed", 30.0).with_source(Source::Tree));
        env.output.set("MovementSpeedMod", 1.30);
        let bd = derive_for(&env, "MovementSpeedMod").unwrap();
        let inc = bd.steps.iter().find(|s| s.label == "Increased").unwrap();
        assert!(inc.sources.iter().any(|s| s.source == "tree"));
    }

    /// Issue #34 follow-up: Life breakdown walks Base → Increased →
    /// Life with item-base + Strength contributions surfaced
    /// separately. Verifies the chain is complete and the Strength
    /// contribution lands in the explain text.
    #[test]
    fn life_breakdown_walks_base_inc_to_total() {
        let env = env_with_output();
        let bd = derive_for(&env, "Life").unwrap();
        let labels: Vec<&str> = bd.steps.iter().map(|s| s.label.as_str()).collect();
        assert!(labels.iter().any(|l| l.starts_with("Base")));
        assert!(labels.iter().any(|l| l.starts_with("Increased")));
        // The Base step's explain string mentions the Strength contribution.
        let base = bd.steps.iter().find(|s| s.label == "Base").unwrap();
        let explain = base.explain.as_deref().unwrap_or("");
        assert!(
            explain.contains("Strength"),
            "Base explain should call out Strength: {explain}"
        );
        assert_eq!(bd.total, 1100.0);
    }

    /// Issue #34 follow-up: Life breakdown surfaces the item-sourced
    /// `+540 Life BASE` mod under the Base step's source list so the
    /// user can see which item contributed.
    #[test]
    fn life_breakdown_sources_carry_through() {
        let env = env_with_output();
        let bd = derive_for(&env, "Life").unwrap();
        let base = bd.steps.iter().find(|s| s.label == "Base").unwrap();
        assert!(
            base.sources
                .iter()
                .any(|s| s.source.contains("item slot 2")),
            "expected item slot 2 source on Base step; got {:?}",
            base.sources,
        );
    }

    /// Issue #34 follow-up: Mana shares the same helper as Life but
    /// keys on Intelligence. Verify the explain string names the
    /// right attribute.
    #[test]
    fn mana_breakdown_uses_intelligence_attribute() {
        let env = env_with_output();
        let bd = derive_for(&env, "Mana").unwrap();
        let base = bd.steps.iter().find(|s| s.label == "Base").unwrap();
        let explain = base.explain.as_deref().unwrap_or("");
        assert!(
            explain.contains("Intelligence"),
            "Mana Base step should call out Intelligence; got {explain}"
        );
    }

    /// Issue #34 follow-up: EnergyShield walks Base → Increased →
    /// Final. Item-base ES (e.g. body armour roll) lands as the Base
    /// contributor; tree INC mods show up under Increased.
    #[test]
    fn energy_shield_breakdown_walks_base_inc_to_total() {
        let mut env = env_with_output();
        env.mod_db
            .add(Mod::base("EnergyShield", 200.0).with_source(Source::Item(2)));
        env.mod_db
            .add(Mod::inc("EnergyShield", 80.0).with_source(Source::Tree));
        env.output.set("EnergyShield", 360.0);
        let bd = derive_for(&env, "EnergyShield").unwrap();
        let labels: Vec<&str> = bd.steps.iter().map(|s| s.label.as_str()).collect();
        assert!(labels.iter().any(|l| l.starts_with("Base")));
        assert!(labels.iter().any(|l| l.starts_with("Increased")));
        let base = bd.steps.iter().find(|s| s.label == "Base").unwrap();
        assert_eq!(base.value, Some(200.0));
        assert!(
            base.sources
                .iter()
                .any(|s| s.source.contains("item slot 2")),
            "expected item slot 2 in Base sources; got {:?}",
            base.sources
        );
        let inc = bd.steps.iter().find(|s| s.label == "Increased").unwrap();
        assert!(inc.sources.iter().any(|s| s.source == "tree"));
        assert_eq!(bd.total, 360.0);
    }

    /// Issue #34 follow-up: empty-pool case — no mods at all on
    /// EnergyShield. The breakdown still emits a Base step (with value
    /// 0) and the final EnergyShield step. The Increased / More steps
    /// are skipped since there's nothing to enumerate.
    #[test]
    fn energy_shield_breakdown_no_mods_still_returns_breakdown() {
        let env = env_with_output();
        let bd = derive_for(&env, "EnergyShield").unwrap();
        let labels: Vec<&str> = bd.steps.iter().map(|s| s.label.as_str()).collect();
        assert_eq!(labels[0], "Base");
        assert_eq!(*labels.last().unwrap(), "EnergyShield");
    }

    /// Issue #34 follow-up: FireResistTotal walks element-specific
    /// BASE → ElementalResist umbrella BASE → level penalty → cap →
    /// effective. Verify each stage lands and the source attribution
    /// preserves item / tree provenance.
    #[test]
    fn fire_resist_breakdown_walks_base_umbrella_penalty_cap() {
        let env = env_with_output();
        let bd = derive_for(&env, "FireResistTotal").unwrap();
        let labels: Vec<&str> = bd.steps.iter().map(|s| s.label.as_str()).collect();
        assert!(labels.iter().any(|l| l.starts_with("Fire Resist BASE")));
        assert!(labels.contains(&"Elemental Resist BASE"));
        assert!(labels.contains(&"Level penalty"));
        assert!(labels.contains(&"Cap"));
        assert!(labels.iter().any(|l| l.contains("(effective)")));
        // Final value matches the env's FireResistTotal output.
        assert_eq!(bd.total, 75.0);
        // Element-specific BASE step shows the item-slot 2 source.
        let base = bd
            .steps
            .iter()
            .find(|s| s.label.starts_with("Fire Resist BASE"))
            .unwrap();
        assert!(
            base.sources
                .iter()
                .any(|s| s.source.contains("item slot 2")),
            "expected item slot 2 source on Fire Resist BASE step; got {:?}",
            base.sources
        );
    }

    /// Issue #34 follow-up: ChaosResistTotal mirrors the elemental
    /// shape but skips the umbrella `ElementalResist` step (chaos
    /// doesn't pick up `+all elemental resistances` mods). Verify
    /// the helper's structure matches.
    #[test]
    fn chaos_resist_breakdown_skips_umbrella_step() {
        let env = env_with_output();
        let bd = derive_for(&env, "ChaosResistTotal").unwrap();
        let labels: Vec<&str> = bd.steps.iter().map(|s| s.label.as_str()).collect();
        assert!(labels.iter().any(|l| l.starts_with("Chaos Resist BASE")));
        // Crucially, the elemental umbrella step does NOT appear for chaos.
        assert!(!labels.contains(&"Elemental Resist BASE"));
        assert!(labels.contains(&"Cap"));
        assert!(labels.iter().any(|l| l.contains("(effective)")));
        // Pinned env: -55 raw, capped at 75.
        assert_eq!(bd.total, -55.0);
    }

    /// Issue #34 follow-up: BlockChance walks BASE → Cap → effective.
    /// The cap step factors in `BlockChanceMax` (75 + tree bonus)
    /// and the BASE step's source list surfaces the shield mod.
    #[test]
    fn block_chance_breakdown_walks_base_cap_effective() {
        let env = env_with_output();
        let bd = derive_for(&env, "BlockChance").unwrap();
        let labels: Vec<&str> = bd.steps.iter().map(|s| s.label.as_str()).collect();
        assert!(labels.iter().any(|l| l.starts_with("Attack Block BASE")));
        assert!(labels.contains(&"Cap"));
        assert!(labels.iter().any(|l| l.contains("(effective)")));
        // Pinned env: 30 raw, cap 75 + 5 = 80, effective 30.
        assert_eq!(bd.total, 30.0);
        let cap = bd.steps.iter().find(|s| s.label == "Cap").unwrap();
        assert_eq!(cap.value, Some(80.0));
        let base = bd
            .steps
            .iter()
            .find(|s| s.label.starts_with("Attack Block BASE"))
            .unwrap();
        assert!(
            base.sources
                .iter()
                .any(|s| s.source.contains("item slot 2")),
            "expected item slot 2 source on Attack Block BASE; got {:?}",
            base.sources
        );
    }

    /// Issue #34 follow-up: SpellBlockChance shares the BlockChanceMax
    /// cap with attack block. Verify the cap step uses the same value.
    #[test]
    fn spell_block_breakdown_shares_block_cap() {
        let env = env_with_output();
        let bd = derive_for(&env, "SpellBlockChance").unwrap();
        let cap = bd.steps.iter().find(|s| s.label == "Cap").unwrap();
        // Cap mirrors BlockChanceMax: 75 + 5 (tree) = 80.
        assert_eq!(cap.value, Some(80.0));
        assert_eq!(bd.total, 20.0);
    }

    /// Issue #34 follow-up: SpellSuppressionChance has a hard 100%
    /// cap (no per-mod cap raise). The cap step's explain should
    /// call this out instead of summing a max-key contribution.
    #[test]
    fn spell_suppression_breakdown_has_hard_cap() {
        let env = env_with_output();
        let bd = derive_for(&env, "SpellSuppressionChance").unwrap();
        let cap = bd.steps.iter().find(|s| s.label == "Cap").unwrap();
        assert_eq!(cap.value, Some(100.0));
        let explain = cap.explain.as_deref().unwrap_or("");
        assert!(
            explain.contains("hard cap"),
            "expected 'hard cap' in cap explain; got {explain}"
        );
        assert_eq!(bd.total, 60.0);
    }

    /// Issue #34 follow-up: when raw block exceeds the cap (e.g.
    /// Glancing Blows or Bringer of Rain stacked over a 5%-bonus
    /// shield mastery), the effective step clamps to the cap and the
    /// explain calls out the `min(raw, cap)` shape.
    #[test]
    fn block_chance_breakdown_clamps_to_cap() {
        let mut env = env_with_output();
        env.mod_db
            .add(Mod::base("BlockChance", 70.0).with_source(Source::Item(3)));
        // Mirror what `perform_basic_stats` would write: raw 100,
        // cap 75 + 5 = 80, effective 80.
        env.output.set("BlockChance", 80.0);
        let bd = derive_for(&env, "BlockChance").unwrap();
        let effective = bd
            .steps
            .iter()
            .find(|s| s.label.contains("(effective)"))
            .unwrap();
        let explain = effective.explain.as_deref().unwrap_or("");
        assert!(
            explain.contains("min(") && explain.contains(", 80)"),
            "expected min(raw, 80) in effective explain; got {explain}"
        );
        assert_eq!(bd.total, 80.0);
    }

    /// Issue #34 follow-up: when a resist isn't capped (e.g. the user
    /// is over-capped at 80% but the cap allows it via gear), the
    /// effective step shows the value clamped to the cap. Test by
    /// raising LightningResist to 100 raw with a default cap of 75 —
    /// the effective value should clamp to 75.
    #[test]
    fn elemental_resist_breakdown_clamps_to_cap() {
        let mut env = env_with_output();
        env.mod_db
            .add(Mod::base("LightningResist", 25.0).with_source(Source::Item(3)));
        // The compute pipeline normally wires this through, but we
        // manually set the output to mirror what perform_basic_stats
        // would write: 75 (existing) + 25 (added) - 60 penalty + 60
        // umbrella = 100 raw, capped at 75.
        env.output.set("LightningResist", 100.0);
        env.output.set("LightningResistTotal", 75.0);
        let bd = derive_for(&env, "LightningResistTotal").unwrap();
        let effective = bd
            .steps
            .iter()
            .find(|s| s.label.contains("(effective)"))
            .unwrap();
        let explain = effective.explain.as_deref().unwrap_or("");
        // The effective explain should call out the min(raw, cap) clamp.
        assert!(
            explain.contains("min(") && explain.contains(", 75)"),
            "expected min(raw, 75) in effective explain; got {explain}"
        );
        assert_eq!(bd.total, 75.0);
    }

    /// Issue #34 follow-up: `MainSkillHitChance` for an attack walks
    /// `Accuracy / Enemy evasion / Hit chance` — the three things the
    /// user can actually move to improve the reading. Spells short-
    /// circuit to a "100% (always hits)" hint instead.
    #[test]
    fn hit_chance_breakdown_attack_surfaces_accuracy_and_enemy_evasion() {
        let mut env = env_with_output();
        env.output.set("Accuracy", 478.0);
        env.output.set("EnemyEvasion", 1500.0);
        env.output.set("MainSkillHitChance", 92.0);
        let bd = derive_for(&env, "MainSkillHitChance").unwrap();
        let labels: Vec<&str> = bd.steps.iter().map(|s| s.label.as_str()).collect();
        assert!(
            labels.contains(&"Accuracy"),
            "missing Accuracy step: {labels:?}"
        );
        assert!(
            labels.contains(&"Enemy evasion"),
            "missing Enemy evasion step: {labels:?}"
        );
        assert!(
            labels.contains(&"Hit chance"),
            "missing Hit chance step: {labels:?}"
        );

        let acc = bd.steps.iter().find(|s| s.label == "Accuracy").unwrap();
        assert_eq!(acc.value, Some(478.0));

        let ev = bd
            .steps
            .iter()
            .find(|s| s.label == "Enemy evasion")
            .unwrap();
        assert_eq!(ev.value, Some(1500.0));

        assert_eq!(bd.total, 92.0);
    }

    /// Issue #34 follow-up: spell-bound builds always hit. The breakdown
    /// must call this out (so the user doesn't go grinding accuracy
    /// for a spell) and still produce a valid 3-step breakdown so the
    /// `covered_keys_is_complete` sweep walks both branches.
    #[test]
    fn hit_chance_breakdown_spell_calls_out_always_hits() {
        let mut env = env_with_output();
        env.output.set("MainSkillHitChance", 100.0);
        let bd = derive_for(&env, "MainSkillHitChance").unwrap();
        let chance = bd.steps.iter().find(|s| s.label == "Hit chance").unwrap();
        assert!(
            chance
                .explain
                .as_deref()
                .unwrap_or("")
                .to_ascii_lowercase()
                .contains("always hits"),
            "spell hit-chance explain should call out always-hits; got {:?}",
            chance.explain
        );
        assert_eq!(bd.total, 100.0);
    }

    /// Issue #34 follow-up: `PhysicalDamageReduction` breakdown shows
    /// the components of PoB's `armour / (armour + 12 × baseline)`
    /// formula against a 1000-pt baseline phys hit, capped at 90%.
    /// Surfaces Armour, the baseline hit constant, and the resulting
    /// reduction so the user can see the diminishing-returns curve
    /// they're climbing.
    #[test]
    fn physical_damage_reduction_breakdown_walks_armour_and_baseline() {
        let env = env_with_output();
        let bd = derive_for(&env, "PhysicalDamageReduction").unwrap();
        let labels: Vec<&str> = bd.steps.iter().map(|s| s.label.as_str()).collect();
        assert!(
            labels.contains(&"Armour"),
            "missing Armour step: {labels:?}"
        );
        assert!(
            labels.contains(&"Baseline phys hit"),
            "missing baseline-hit step: {labels:?}"
        );

        let armour = bd.steps.iter().find(|s| s.label == "Armour").unwrap();
        assert_eq!(armour.value, Some(4500.0));

        let baseline = bd
            .steps
            .iter()
            .find(|s| s.label == "Baseline phys hit")
            .unwrap();
        assert_eq!(baseline.value, Some(1000.0));

        // Final reading is the stored output value (27.27% for the
        // fixture's 4500 armour vs 1000 baseline).
        assert!(
            (bd.total - 27.27).abs() < 1e-6,
            "expected total ≈ 27.27; got {}",
            bd.total
        );
    }

    /// Issue #34 follow-up: hard-cap surfacing — at very high armour
    /// the formula caps at 90%. Verify the breakdown reports that
    /// cap and labels the explain text appropriately so the user
    /// knows further armour stacking is wasted.
    #[test]
    fn physical_damage_reduction_breakdown_calls_out_90_percent_cap() {
        let mut env = env_with_output();
        env.output.set("Armour", 200_000.0);
        env.output.set("PhysicalDamageReduction", 90.0);
        let bd = derive_for(&env, "PhysicalDamageReduction").unwrap();
        let final_step = bd
            .steps
            .iter()
            .find(|s| s.label == "PhysicalDamageReduction")
            .unwrap();
        assert!(
            final_step
                .explain
                .as_deref()
                .unwrap_or("")
                .to_ascii_lowercase()
                .contains("cap"),
            "expected cap callout in final-step explain; got {:?}",
            final_step.explain
        );
        assert_eq!(bd.total, 90.0);
    }

    /// Issue #34 follow-up: `AverageEHP` is the simple arithmetic mean
    /// of the five per-element EHPs. The breakdown surfaces all five
    /// contributors so the user can see which damage types drag the
    /// reading down.
    #[test]
    fn average_ehp_breakdown_shows_all_five_contributors() {
        let env = env_with_output();
        let bd = derive_for(&env, "AverageEHP").unwrap();
        let labels: Vec<&str> = bd.steps.iter().map(|s| s.label.as_str()).collect();
        // One step per element + one final.
        for elem in ["Physical", "Fire", "Cold", "Lightning", "Chaos"] {
            assert!(
                labels.contains(&elem),
                "missing {elem} contributor: {labels:?}"
            );
        }
        assert!((bd.total - 4894.6).abs() < 0.1, "got {}", bd.total);
    }

    /// Issue #34 follow-up: `MinimumEHP` highlights the worst-case
    /// damage type. The breakdown calls out which element is the
    /// weakest in the explain text so the user can target their
    /// defensive investment.
    #[test]
    fn minimum_ehp_breakdown_calls_out_worst_case_element() {
        let env = env_with_output();
        let bd = derive_for(&env, "MinimumEHP").unwrap();
        // The worst case in the fixture is Chaos at 1267.
        let final_step = bd
            .steps
            .iter()
            .find(|s| s.label == "MinimumEHP")
            .expect("missing final step");
        assert!(
            final_step
                .explain
                .as_deref()
                .unwrap_or("")
                .to_ascii_lowercase()
                .contains("chaos"),
            "expected Chaos call-out in MinimumEHP explain; got {:?}",
            final_step.explain
        );
        assert_eq!(bd.total, 1267.0);
    }

    /// Issue #34 follow-up: `FireEHP` shows pool, the resist factor
    /// (after the 3% enemy pen the perform pass bakes in for the
    /// standard Pinnacle Boss preset), the spell-suppression factor,
    /// the spell-block factor, and the final reading. All three
    /// elemental EHPs share one helper; chaos uses the same shape
    /// without the 3% pen.
    #[test]
    fn fire_ehp_breakdown_walks_pool_resist_suppress_block() {
        let env = env_with_output();
        let bd = derive_for(&env, "FireEHP").unwrap();
        let labels: Vec<&str> = bd.steps.iter().map(|s| s.label.as_str()).collect();
        assert!(labels.contains(&"Pool"), "missing Pool: {labels:?}");
        assert!(
            labels.contains(&"Fire resist"),
            "missing Fire resist: {labels:?}"
        );
        assert!(
            labels.contains(&"Spell suppression"),
            "missing Spell suppression: {labels:?}"
        );
        assert!(
            labels.contains(&"Spell block"),
            "missing Spell block: {labels:?}"
        );

        let pool = bd.steps.iter().find(|s| s.label == "Pool").unwrap();
        assert_eq!(pool.value, Some(1100.0));

        // Fire resist factor with 3% pen: 1 - (0.75 - 0.03) = 0.28
        let resist = bd.steps.iter().find(|s| s.label == "Fire resist").unwrap();
        assert!(
            (resist.value.unwrap_or(0.0) - 0.28).abs() < 0.001,
            "expected fire factor ~0.28; got {:?}",
            resist.value
        );

        // Spell suppression: 1 - 0.5 × 0.6 = 0.7
        let suppress = bd
            .steps
            .iter()
            .find(|s| s.label == "Spell suppression")
            .unwrap();
        assert!(
            (suppress.value.unwrap_or(0.0) - 0.7).abs() < 1e-6,
            "expected suppress factor 0.7; got {:?}",
            suppress.value
        );

        // Spell block: 1 - 0.20 = 0.8
        let sblock = bd.steps.iter().find(|s| s.label == "Spell block").unwrap();
        assert!(
            (sblock.value.unwrap_or(0.0) - 0.8).abs() < 1e-6,
            "expected spell-block factor 0.8; got {:?}",
            sblock.value
        );

        assert_eq!(bd.total, 7015.0);
    }

    /// Issue #34 follow-up: chaos shares the elemental shape but
    /// uses no enemy penetration (PoB's standard preset has no chaos
    /// pen). With ChaosResistTotal = -55 the resist factor is 1.55,
    /// not the 0.28 the elemental side gets — pinning that the helper
    /// reads the correct resist key.
    #[test]
    fn chaos_ehp_breakdown_skips_enemy_pen_uses_chaos_resist() {
        let env = env_with_output();
        let bd = derive_for(&env, "ChaosEHP").unwrap();
        let resist = bd.steps.iter().find(|s| s.label == "Chaos resist").unwrap();
        // Chaos at -55%, no pen: 1 - (-0.55) = 1.55
        assert!(
            (resist.value.unwrap_or(0.0) - 1.55).abs() < 1e-6,
            "expected chaos factor 1.55; got {:?}",
            resist.value
        );
        assert_eq!(bd.total, 1267.0);
    }

    /// Issue #34 follow-up: `PhysicalEHP` shows pool and the
    /// physical-damage-taken multiplier components — the
    /// 1-stop-shop for "how much physical damage can I eat".
    #[test]
    fn physical_ehp_breakdown_walks_pool_phys_red_block_to_total() {
        let env = env_with_output();
        let bd = derive_for(&env, "PhysicalEHP").unwrap();
        let labels: Vec<&str> = bd.steps.iter().map(|s| s.label.as_str()).collect();
        assert!(labels.contains(&"Pool"), "missing Pool step: {labels:?}");
        assert!(
            labels.contains(&"Physical reduction"),
            "missing phys-red step: {labels:?}"
        );
        assert!(labels.contains(&"Block"), "missing Block step: {labels:?}");

        let pool = bd.steps.iter().find(|s| s.label == "Pool").unwrap();
        // Life=1100 + ES=0 + Ward=0 = 1100.
        assert_eq!(pool.value, Some(1100.0));

        // Phys-red factor = 1 - 0.2727 ≈ 0.7273
        let red = bd
            .steps
            .iter()
            .find(|s| s.label == "Physical reduction")
            .unwrap();
        assert!(
            (red.value.unwrap_or(0.0) - 0.7273).abs() < 0.001,
            "expected phys-red factor ~0.7273; got {:?}",
            red.value
        );

        // Block factor = 1 - 0.3 = 0.7
        let block = bd.steps.iter().find(|s| s.label == "Block").unwrap();
        assert!(
            (block.value.unwrap_or(0.0) - 0.7).abs() < 1e-6,
            "expected block factor 0.7; got {:?}",
            block.value
        );

        assert_eq!(bd.total, 2161.0);
    }

    /// Issue #34 follow-up: `BleedDPS` walks per-application damage ×
    /// chance. Bleed is single-stack (longest overrides), so the
    /// long-run DPS is `chance × per_application`. Players tuning
    /// chance-to-bleed gear / Crimson Dance jewels want to see both
    /// axes. Per-application back-derived from `BleedDPS / chance`
    /// since the perform pass doesn't store it.
    #[test]
    fn bleed_dps_breakdown_walks_per_application_and_chance() {
        let mut env = env_with_output();
        env.output.set("BleedDPS", 800.0);
        env.output.set("BleedChance", 25.0);
        let bd = derive_for(&env, "BleedDPS").unwrap();
        let labels: Vec<&str> = bd.steps.iter().map(|s| s.label.as_str()).collect();
        assert!(
            labels.contains(&"Per-application damage"),
            "missing per-app step: {labels:?}"
        );
        assert!(
            labels.contains(&"Bleed chance"),
            "missing chance step: {labels:?}"
        );

        // Per-application = 800 / 0.25 = 3200.
        let per_app = bd
            .steps
            .iter()
            .find(|s| s.label == "Per-application damage")
            .unwrap();
        assert_eq!(per_app.value, Some(3200.0));

        // Chance step value carries the factor (0.25), percent in explain.
        let chance = bd.steps.iter().find(|s| s.label == "Bleed chance").unwrap();
        assert!(
            (chance.value.unwrap_or(0.0) - 0.25).abs() < 1e-6,
            "expected chance factor 0.25; got {:?}",
            chance.value
        );

        assert_eq!(bd.total, 800.0);
    }

    /// Issue #34 follow-up: `IgniteDPS` shares the bleed shape — the
    /// helper handles both via the same code path with element-
    /// specific labels. Ignite chance is often 100% (crit-ignite or
    /// Avatar of Fire builds); the breakdown reads the chance and
    /// degenerates the per-application value to the DPS itself.
    #[test]
    fn ignite_dps_breakdown_walks_per_application_and_chance() {
        let mut env = env_with_output();
        env.output.set("IgniteDPS", 1500.0);
        env.output.set("IgniteChance", 100.0);
        let bd = derive_for(&env, "IgniteDPS").unwrap();
        let per_app = bd
            .steps
            .iter()
            .find(|s| s.label == "Per-application damage")
            .unwrap();
        // 1500 / 1.0 = 1500.
        assert_eq!(per_app.value, Some(1500.0));
        assert_eq!(bd.total, 1500.0);
    }

    /// Issue #34 follow-up: zero-DPS / zero-chance build still
    /// produces a valid breakdown (covered_keys_is_complete sweep).
    #[test]
    fn bleed_and_ignite_dps_breakdowns_handle_zero_chance() {
        let env = env_with_output();
        for key in ["BleedDPS", "IgniteDPS"] {
            let bd = derive_for(&env, key).unwrap_or_else(|| panic!("missing for {key}"));
            assert_eq!(bd.total, 0.0);
            assert_eq!(bd.steps.len(), 3, "wrong step count for {key}");
        }
    }

    /// Issue #34 follow-up: `PoisonDPS` walks per-stack damage ×
    /// steady-state stack count. Players tuning poison-stacking
    /// builds want to see whether their DPS comes from raw per-hit
    /// damage or from stack-count investment (cast speed, duration,
    /// chance). Per-stack damage is back-derived from PoisonDPS /
    /// PoisonStacks since the perform pass doesn't store it.
    #[test]
    fn poison_dps_breakdown_walks_per_stack_and_stacks() {
        let env = env_with_output();
        let bd = derive_for(&env, "PoisonDPS").unwrap();
        let labels: Vec<&str> = bd.steps.iter().map(|s| s.label.as_str()).collect();
        assert!(
            labels.contains(&"Per-stack damage"),
            "missing per-stack: {labels:?}"
        );
        assert!(
            labels.contains(&"Steady-state stacks"),
            "missing stacks: {labels:?}"
        );

        let per_stack = bd
            .steps
            .iter()
            .find(|s| s.label == "Per-stack damage")
            .unwrap();
        assert_eq!(per_stack.value, Some(35.0));

        let stacks = bd
            .steps
            .iter()
            .find(|s| s.label == "Steady-state stacks")
            .unwrap();
        assert_eq!(stacks.value, Some(10.0));

        assert_eq!(bd.total, 350.0);
    }

    /// Issue #34 follow-up: builds with no poison output (`PoisonDPS = 0`
    /// or `PoisonStacks = 0`) still produce a valid breakdown so the
    /// `covered_keys_is_complete` sweep walks the dispatch arm. Per-stack
    /// degenerates gracefully to 0.
    #[test]
    fn poison_dps_breakdown_handles_zero_dps() {
        let mut env = env_with_output();
        env.output.set("PoisonDPS", 0.0);
        env.output.set("PoisonStacks", 0.0);
        let bd = derive_for(&env, "PoisonDPS").unwrap();
        assert_eq!(bd.total, 0.0);
        let per_stack = bd
            .steps
            .iter()
            .find(|s| s.label == "Per-stack damage")
            .unwrap();
        assert_eq!(per_stack.value, Some(0.0));
    }

    /// Issue #34 follow-up: `ImpaleDPS` walks the four knobs that
    /// drive impale damage — stored hit average, per-stack effect,
    /// impale chance, and cast/swing speed. Stack count is a fixed
    /// 5 by default (raised by `ImpaleStacksMax`); the breakdown
    /// surfaces it inside the explain text rather than as its own
    /// step since most builds don't tune it.
    #[test]
    fn impale_dps_breakdown_walks_stored_effect_chance_speed() {
        let env = env_with_output();
        let bd = derive_for(&env, "ImpaleDPS").unwrap();
        let labels: Vec<&str> = bd.steps.iter().map(|s| s.label.as_str()).collect();
        for needed in [
            "Stored hit average",
            "Effect per stack",
            "Impale chance",
            "Casts per second",
        ] {
            assert!(
                labels.contains(&needed),
                "missing {needed} step: {labels:?}"
            );
        }

        let stored = bd
            .steps
            .iter()
            .find(|s| s.label == "Stored hit average")
            .unwrap();
        assert_eq!(stored.value, Some(400.0));

        let effect = bd
            .steps
            .iter()
            .find(|s| s.label == "Effect per stack")
            .unwrap();
        // Effect step value carries the factor (0.10), percent in explain.
        assert!(
            (effect.value.unwrap_or(0.0) - 0.10).abs() < 1e-6,
            "expected effect factor 0.10; got {:?}",
            effect.value
        );

        let chance = bd
            .steps
            .iter()
            .find(|s| s.label == "Impale chance")
            .unwrap();
        assert!(
            (chance.value.unwrap_or(0.0) - 1.0).abs() < 1e-6,
            "expected chance factor 1.0; got {:?}",
            chance.value
        );

        assert_eq!(bd.total, 1000.0);
    }

    /// Issue #34 follow-up: spell / non-impale build (zero impale
    /// chance) still produces a valid breakdown so the
    /// `covered_keys_is_complete` sweep walks the dispatch arm.
    #[test]
    fn impale_dps_breakdown_handles_zero_chance() {
        let mut env = env_with_output();
        env.output.set("ImpaleChance", 0.0);
        env.output.set("ImpaleDPS", 0.0);
        env.output.set("ImpaleStoredHitAvg", 0.0);
        env.output.set("ImpaleEffect", 0.0);
        let bd = derive_for(&env, "ImpaleDPS").unwrap();
        assert_eq!(bd.total, 0.0);
    }

    /// Issue #34 follow-up: `TotalEHP` is PoB's headline defensive
    /// number — `hits_to_die × totalEnemyDamageIn`. Distinct from
    /// per-element EHP because it folds the Pinnacle Boss's mixed
    /// damage profile into a single number. Two-component breakdown.
    #[test]
    fn total_ehp_breakdown_walks_hits_and_damage_in() {
        let env = env_with_output();
        let bd = derive_for(&env, "TotalEHP").unwrap();
        let labels: Vec<&str> = bd.steps.iter().map(|s| s.label.as_str()).collect();
        assert!(
            labels.contains(&"Hits to die"),
            "missing hits step: {labels:?}"
        );
        assert!(
            labels.contains(&"Boss damage per hit"),
            "missing damage-in step: {labels:?}"
        );

        let hits = bd.steps.iter().find(|s| s.label == "Hits to die").unwrap();
        assert_eq!(hits.value, Some(4.0));

        let damage_in = bd
            .steps
            .iter()
            .find(|s| s.label == "Boss damage per hit")
            .unwrap();
        assert_eq!(damage_in.value, Some(1500.0));

        assert_eq!(bd.total, 6000.0);
    }

    /// Issue #34 follow-up: `EHPSurvivalTime` answers "how many
    /// seconds before the standard boss kills me". Two-component:
    /// hits-to-die × the boss's tick rate. Lets the user reason
    /// about whether to invest in raw EHP or in mobility / dodge
    /// affixes.
    #[test]
    fn ehp_survival_time_breakdown_walks_hits_and_tick_rate() {
        let env = env_with_output();
        let bd = derive_for(&env, "EHPSurvivalTime").unwrap();
        let labels: Vec<&str> = bd.steps.iter().map(|s| s.label.as_str()).collect();
        assert!(
            labels.contains(&"Hits to die"),
            "missing hits step: {labels:?}"
        );
        assert!(
            labels.contains(&"Boss tick"),
            "missing tick step: {labels:?}"
        );

        let hits = bd.steps.iter().find(|s| s.label == "Hits to die").unwrap();
        assert_eq!(hits.value, Some(4.0));

        let tick = bd.steps.iter().find(|s| s.label == "Boss tick").unwrap();
        assert!(
            (tick.value.unwrap_or(0.0) - 0.7).abs() < 1e-6,
            "expected tick 0.7; got {:?}",
            tick.value
        );

        assert!((bd.total - 2.8).abs() < 1e-6, "got {}", bd.total);
    }

    /// Issue #34 follow-up: `NumberOfDamagingHits` answers "how many
    /// hits before I die". `pool / total_taken_hit` — defensive
    /// builds tuning EHP investment want to see whether their
    /// hits-to-die figure is constrained by pool or by mitigation.
    #[test]
    fn number_of_damaging_hits_breakdown_walks_pool_and_taken() {
        let env = env_with_output();
        let bd = derive_for(&env, "NumberOfDamagingHits").unwrap();
        let labels: Vec<&str> = bd.steps.iter().map(|s| s.label.as_str()).collect();
        assert!(labels.contains(&"Pool"), "missing Pool: {labels:?}");
        assert!(
            labels.contains(&"Total taken per hit"),
            "missing taken: {labels:?}"
        );

        let pool = bd.steps.iter().find(|s| s.label == "Pool").unwrap();
        assert_eq!(pool.value, Some(1100.0));

        let taken = bd
            .steps
            .iter()
            .find(|s| s.label == "Total taken per hit")
            .unwrap();
        assert_eq!(taken.value, Some(275.0));

        assert_eq!(bd.total, 4.0);
    }

    /// Issue #34 follow-up: `ManaPerSecondCost` = mana_cost × cps.
    /// Spell builds tuning sustain want to see whether their mana
    /// burn is driven by per-cast cost or by cast speed — the
    /// breakdown surfaces both contributors.
    #[test]
    fn mana_per_second_cost_breakdown_walks_cost_and_cps() {
        let env = env_with_output();
        let bd = derive_for(&env, "ManaPerSecondCost").unwrap();
        let labels: Vec<&str> = bd.steps.iter().map(|s| s.label.as_str()).collect();
        assert!(
            labels.contains(&"Mana cost"),
            "missing Mana cost: {labels:?}"
        );
        assert!(
            labels.contains(&"Casts per second"),
            "missing cps: {labels:?}"
        );

        let cost = bd.steps.iter().find(|s| s.label == "Mana cost").unwrap();
        assert_eq!(cost.value, Some(12.0));

        let cps = bd
            .steps
            .iter()
            .find(|s| s.label == "Casts per second")
            .unwrap();
        assert_eq!(cps.value, Some(5.0));

        assert_eq!(bd.total, 60.0);
    }

    /// Issue #34 follow-up: `MainSkillManaCost` walks
    /// `base × (1 + Inc%)`. Spell builds tuning Lifetap / mana-cost
    /// reservation chains need to see how much of their final cost
    /// comes from cost-reduction mods vs the gem's own base value.
    /// The base is back-derived from `total / (1 + Inc/100)` so the
    /// helper doesn't need to re-run the perform pass.
    #[test]
    fn main_skill_mana_cost_breakdown_walks_base_inc_to_final() {
        let env = env_with_output();
        let bd = derive_for(&env, "MainSkillManaCost").unwrap();
        let labels: Vec<&str> = bd.steps.iter().map(|s| s.label.as_str()).collect();
        assert!(
            labels.contains(&"Base mana cost"),
            "missing base step: {labels:?}"
        );
        assert!(
            labels.contains(&"Increased / Reduced"),
            "missing inc/red step: {labels:?}"
        );

        // Back-derived base: 12 / (1 + (-25)/100) = 12 / 0.75 = 16.
        let base = bd
            .steps
            .iter()
            .find(|s| s.label == "Base mana cost")
            .unwrap();
        assert!(
            (base.value.unwrap_or(0.0) - 16.0).abs() < 1e-6,
            "expected back-derived base 16; got {:?}",
            base.value
        );

        // Inc step value carries the multiplier (0.75) so the chain
        // reads multiplicatively. Percent surfaced in explain text.
        let inc = bd
            .steps
            .iter()
            .find(|s| s.label == "Increased / Reduced")
            .unwrap();
        assert!(
            (inc.value.unwrap_or(0.0) - 0.75).abs() < 1e-6,
            "expected mult 0.75; got {:?}",
            inc.value
        );

        assert_eq!(bd.total, 12.0);
    }

    /// Issue #34 follow-up: builds with no cost-reduction mods see a
    /// 100% multiplier and the breakdown still produces a valid 3-step
    /// view (so `covered_keys_is_complete` walks both branches).
    #[test]
    fn main_skill_mana_cost_breakdown_no_reduction_passes_through() {
        let mut env = env_with_output();
        env.mod_db = crate::ModDB::default();
        env.output.set("MainSkillManaCost", 16.0);
        let bd = derive_for(&env, "MainSkillManaCost").unwrap();
        let inc = bd
            .steps
            .iter()
            .find(|s| s.label == "Increased / Reduced")
            .unwrap();
        assert!(
            (inc.value.unwrap_or(0.0) - 1.0).abs() < 1e-6,
            "expected 1.0 with no cost mods; got {:?}",
            inc.value
        );
        let base = bd
            .steps
            .iter()
            .find(|s| s.label == "Base mana cost")
            .unwrap();
        assert!(
            (base.value.unwrap_or(0.0) - 16.0).abs() < 1e-6,
            "expected base 16; got {:?}",
            base.value
        );
        assert_eq!(bd.total, 16.0);
    }

    /// Issue #34 follow-up: `WithPoisonDPS` is the simple sum of hit
    /// DPS plus poison DPS. The breakdown surfaces both contributors
    /// so the user can see how much of their total comes from the
    /// hit vs the poison ailment stack.
    #[test]
    fn with_poison_dps_breakdown_walks_hit_and_poison() {
        let env = env_with_output();
        let bd = derive_for(&env, "WithPoisonDPS").unwrap();
        let labels: Vec<&str> = bd.steps.iter().map(|s| s.label.as_str()).collect();
        assert!(
            labels.contains(&"Hit DPS"),
            "missing hit DPS step: {labels:?}"
        );
        assert!(
            labels.contains(&"Poison DPS"),
            "missing poison DPS step: {labels:?}"
        );

        let hit = bd.steps.iter().find(|s| s.label == "Hit DPS").unwrap();
        assert_eq!(hit.value, Some(1650.0));

        let poison = bd.steps.iter().find(|s| s.label == "Poison DPS").unwrap();
        assert_eq!(poison.value, Some(350.0));

        assert_eq!(bd.total, 2000.0);
    }

    /// Issue #34 follow-up: zero-ailment cases still produce a valid
    /// breakdown — the ailment step just shows 0. Without this the
    /// covered_keys_is_complete sweep would fail on every build that
    /// doesn't run the matching ailment.
    #[test]
    fn with_bleed_ignite_impale_dps_breakdowns_collapse_to_hit_when_zero() {
        let env = env_with_output();
        for key in ["WithBleedDPS", "WithIgniteDPS", "WithImpaleDPS"] {
            let bd = derive_for(&env, key).unwrap_or_else(|| panic!("missing breakdown for {key}"));
            assert_eq!(bd.total, 1650.0, "wrong total for {key}");
            assert_eq!(
                bd.steps.len(),
                3,
                "wrong step count for {key}: {:?}",
                bd.steps
            );
        }
    }

    /// Issue #34 follow-up: `MainSkillAverageHitAfterAccuracy`
    /// completes the damage chain — `AfterShock × HitChance/100`.
    /// Spells always hit at 100%, so the breakdown surfaces a
    /// "100% (always hits)" hint and the value passes through
    /// unchanged; attacks see the actual hit-chance percentage and
    /// the multiplicative drop.
    #[test]
    fn after_accuracy_breakdown_attack_applies_hit_chance() {
        let mut env = env_with_output();
        env.output.set("MainSkillAverageHitAfterShock", 400.0);
        env.output.set("MainSkillHitChance", 80.0);
        env.output.set("MainSkillAverageHitAfterAccuracy", 320.0);
        let bd = derive_for(&env, "MainSkillAverageHitAfterAccuracy").unwrap();
        let labels: Vec<&str> = bd.steps.iter().map(|s| s.label.as_str()).collect();
        assert!(
            labels.contains(&"After shock"),
            "missing after-shock step: {labels:?}"
        );
        assert!(
            labels.contains(&"Hit chance"),
            "missing hit-chance step: {labels:?}"
        );

        let after_shock = bd.steps.iter().find(|s| s.label == "After shock").unwrap();
        assert_eq!(after_shock.value, Some(400.0));

        // Hit-chance step value carries the multiplier (0.80), not
        // the raw percent — same shape as the resist-factor step.
        let hit = bd.steps.iter().find(|s| s.label == "Hit chance").unwrap();
        assert!(
            (hit.value.unwrap_or(0.0) - 0.8).abs() < 1e-6,
            "expected hit-chance factor 0.8; got {:?}",
            hit.value
        );

        assert_eq!(bd.total, 320.0);
    }

    /// Issue #34 follow-up: spells always hit. The hit-chance step's
    /// `explain` must call this out so the user doesn't read a
    /// `1.0×` factor as "I should invest in accuracy" on a spell
    /// build.
    #[test]
    fn after_accuracy_breakdown_spell_calls_out_always_hits() {
        let mut env = env_with_output();
        env.output.set("MainSkillAverageHitAfterShock", 400.0);
        env.output.set("MainSkillHitChance", 100.0);
        env.output.set("MainSkillAverageHitAfterAccuracy", 400.0);
        let bd = derive_for(&env, "MainSkillAverageHitAfterAccuracy").unwrap();
        let hit = bd.steps.iter().find(|s| s.label == "Hit chance").unwrap();
        assert!(
            hit.explain
                .as_deref()
                .unwrap_or("")
                .to_ascii_lowercase()
                .contains("always hits"),
            "spell hit-chance explain should call out always-hits; got {:?}",
            hit.explain
        );
        assert_eq!(bd.total, 400.0);
    }

    /// Issue #34 follow-up: `MainSkillAverageHitAfterShock` walks the
    /// chain `AfterResist × ShockMult (+ PhysGainAsExtraDamage)` →
    /// AfterShock. Shock-less builds see `ShockMult = 1.0` and a
    /// no-op step; shocked builds see the multiplier surfaced. The
    /// gain-as-extra step is only emitted when non-zero (most spell
    /// builds, all unmodded attacks).
    #[test]
    fn after_shock_breakdown_walks_after_resist_and_shock_mult() {
        let mut env = env_with_output();
        env.output.set("MainSkillAverageHitAfterResist", 280.0);
        env.output.set("MainSkillShockMult", 1.30);
        env.output.set("PhysicalGainAsExtraDamage", 0.0);
        env.output.set("MainSkillAverageHitAfterShock", 364.0); // 280 × 1.30
        let bd = derive_for(&env, "MainSkillAverageHitAfterShock").unwrap();
        let labels: Vec<&str> = bd.steps.iter().map(|s| s.label.as_str()).collect();
        assert!(
            labels.contains(&"After resist"),
            "missing after-resist step: {labels:?}"
        );
        assert!(
            labels.contains(&"Shock multiplier"),
            "missing shock-mult step: {labels:?}"
        );
        // Gain-as-extra is zero so its step is omitted to keep the
        // panel tidy on the common case.
        assert!(
            !labels.contains(&"Phys gain-as extra"),
            "spurious gain-as step on zero value: {labels:?}"
        );

        let after_res = bd.steps.iter().find(|s| s.label == "After resist").unwrap();
        assert_eq!(after_res.value, Some(280.0));

        let shock = bd
            .steps
            .iter()
            .find(|s| s.label == "Shock multiplier")
            .unwrap();
        assert!((shock.value.unwrap_or(0.0) - 1.30).abs() < 1e-6);

        assert_eq!(bd.total, 364.0);
    }

    /// Issue #34 follow-up: a non-zero `PhysicalGainAsExtraDamage`
    /// surfaces as its own step — this is how a phys-attack build
    /// stacking gain-as elemental sees the extra damage layer.
    #[test]
    fn after_shock_breakdown_surfaces_phys_gain_as_extra_when_present() {
        let mut env = env_with_output();
        env.output.set("MainSkillAverageHitAfterResist", 280.0);
        env.output.set("MainSkillShockMult", 1.0);
        env.output.set("PhysicalGainAsExtraDamage", 50.0);
        env.output.set("MainSkillAverageHitAfterShock", 330.0); // 280 + 50
        let bd = derive_for(&env, "MainSkillAverageHitAfterShock").unwrap();
        let gain_as = bd
            .steps
            .iter()
            .find(|s| s.label == "Phys gain-as extra")
            .unwrap_or_else(|| panic!("missing phys gain-as step: {:?}", bd.steps));
        assert_eq!(gain_as.value, Some(50.0));
        assert_eq!(bd.total, 330.0);
    }

    /// Issue #34 follow-up: `MainSkillAverageHitAfterResist` shows
    /// the multiplicative chain `AverageHitWithCrit × (1 - eff_resist/100)`
    /// — what the user reads as "your hit, after the enemy's resist".
    /// The breakdown surfaces the source hit, the resist multiplier
    /// (with the percent value), and the final number.
    #[test]
    fn after_resist_breakdown_walks_avg_with_crit_then_resist_factor() {
        let mut env = env_with_output();
        // Re-pin the chain numbers so the test is independent of the
        // fixture's representative-but-not-consistent baseline:
        // AvgWithCrit = 400, EffectiveResist = 30%, AfterResist = 280.
        env.output.set("MainSkillAverageHitWithCrit", 400.0);
        env.output.set("MainSkillEnemyEffectiveResist", 30.0);
        env.output.set("MainSkillAverageHitAfterResist", 280.0);
        let bd = derive_for(&env, "MainSkillAverageHitAfterResist").unwrap();
        let labels: Vec<&str> = bd.steps.iter().map(|s| s.label.as_str()).collect();
        assert!(
            labels.contains(&"Average hit (with crit)"),
            "missing avg-hit step: {labels:?}"
        );
        assert!(
            labels.contains(&"Enemy resist factor"),
            "missing resist factor step: {labels:?}"
        );

        let avg = bd
            .steps
            .iter()
            .find(|s| s.label == "Average hit (with crit)")
            .unwrap();
        assert_eq!(avg.value, Some(400.0));

        // Resist factor = 1 - 30/100 = 0.7.
        let resist = bd
            .steps
            .iter()
            .find(|s| s.label == "Enemy resist factor")
            .unwrap();
        assert!(
            (resist.value.unwrap_or(0.0) - 0.7).abs() < 1e-6,
            "expected resist factor 0.7; got {:?}",
            resist.value
        );

        assert_eq!(bd.total, 280.0);
    }

    /// Issue #34 follow-up: Accuracy walks Base mods → Level → Dex →
    /// Final, surfacing the three contributors PoB's formula has
    /// (`Σ BASE(Accuracy) + 2(level-1) + 2 × Dex`). The level term is
    /// back-derived from `total - mod_acc - dex_term` so the helper
    /// doesn't need direct access to `Character::level`. Item-sourced
    /// `+200 Accuracy` lands under the Base step's source list.
    #[test]
    fn accuracy_breakdown_walks_base_level_dex_to_total() {
        let env = env_with_output();
        let bd = derive_for(&env, "Accuracy").unwrap();
        let labels: Vec<&str> = bd.steps.iter().map(|s| s.label.as_str()).collect();
        assert!(
            labels.contains(&"Base mods"),
            "missing Base mods step: {labels:?}"
        );
        assert!(labels.contains(&"Level"), "missing Level step: {labels:?}");
        assert!(
            labels.contains(&"Dexterity"),
            "missing Dexterity step: {labels:?}"
        );

        let base = bd.steps.iter().find(|s| s.label == "Base mods").unwrap();
        assert_eq!(base.value, Some(200.0));
        assert!(
            base.sources
                .iter()
                .any(|s| s.source.contains("item slot 2")),
            "expected item slot 2 source on Accuracy Base mods step; got {:?}",
            base.sources
        );

        // Dex term: 2 × 50 = 100.
        let dex = bd.steps.iter().find(|s| s.label == "Dexterity").unwrap();
        assert_eq!(dex.value, Some(100.0));

        // Level term back-derived: 478 - 200 - 100 = 178.
        let level = bd.steps.iter().find(|s| s.label == "Level").unwrap();
        assert_eq!(level.value, Some(178.0));

        assert_eq!(bd.total, 478.0);
    }

    /// Issue #34 follow-up: Strength walks Base → Final, enumerating
    /// the class-start + item BASE sources. Attributes use the same
    /// `pool_basic` shape as Armour / Evasion / Ward — the test pins
    /// the dispatch routing and the source-enumeration contract.
    #[test]
    fn strength_breakdown_walks_base_to_total() {
        let env = env_with_output();
        let bd = derive_for(&env, "Strength").unwrap();
        let labels: Vec<&str> = bd.steps.iter().map(|s| s.label.as_str()).collect();
        assert!(labels.contains(&"Base"));
        let base = bd.steps.iter().find(|s| s.label == "Base").unwrap();
        // Base step value should sum the two BASE mods we seeded.
        assert_eq!(base.value, Some(80.0));
        // Both source labels should appear under the Base step.
        assert!(
            base.sources
                .iter()
                .any(|s| s.source.contains("class start")),
            "expected class-start source on Strength Base step; got {:?}",
            base.sources
        );
        assert!(
            base.sources
                .iter()
                .any(|s| s.source.contains("item slot 2")),
            "expected item slot 2 source on Strength Base step; got {:?}",
            base.sources
        );
        assert_eq!(bd.total, 80.0);
    }

    /// Issue #34 follow-up: Dexterity / Intelligence route through the
    /// same `pool_basic` helper as Strength. Pin the routing so a
    /// future dispatch refactor can't silently drop them.
    #[test]
    fn dexterity_and_intelligence_breakdowns_route_through_pool_basic() {
        let env = env_with_output();
        let dex = derive_for(&env, "Dexterity").unwrap();
        assert_eq!(dex.total, 50.0);
        assert!(
            dex.steps.iter().any(|s| s.label == "Base"),
            "Dexterity breakdown missing Base step",
        );

        let int_ = derive_for(&env, "Intelligence").unwrap();
        assert_eq!(int_.total, 60.0);
        assert!(
            int_.steps.iter().any(|s| s.label == "Base"),
            "Intelligence breakdown missing Base step",
        );
    }

    /// Issue #34 follow-up: Armour walks Base → Increased → Final.
    /// Strength-sourced INC Armour shows up under Increased; the
    /// item-slot 2 BASE survives in the Base step's source list.
    #[test]
    fn armour_breakdown_walks_base_inc_to_total() {
        let env = env_with_output();
        let bd = derive_for(&env, "Armour").unwrap();
        let labels: Vec<&str> = bd.steps.iter().map(|s| s.label.as_str()).collect();
        assert!(labels.contains(&"Base"));
        assert!(labels.contains(&"Increased"));
        let base = bd.steps.iter().find(|s| s.label == "Base").unwrap();
        assert!(
            base.sources
                .iter()
                .any(|s| s.source.contains("item slot 2")),
            "expected item slot 2 source on Armour Base step; got {:?}",
            base.sources
        );
        let inc = bd.steps.iter().find(|s| s.label == "Increased").unwrap();
        assert!(
            inc.sources.iter().any(|s| s.source.contains("Strength")),
            "expected Strength source on Armour Increased step; got {:?}",
            inc.sources
        );
        assert_eq!(bd.total, 4500.0);
    }

    /// Issue #34 follow-up: Evasion mirrors Armour but Dexterity →
    /// Inc Evasion lands as the attribute INC source.
    #[test]
    fn evasion_breakdown_dex_sourced_inc() {
        let env = env_with_output();
        let bd = derive_for(&env, "Evasion").unwrap();
        let inc = bd.steps.iter().find(|s| s.label == "Increased").unwrap();
        assert!(
            inc.sources.iter().any(|s| s.source.contains("Dexterity")),
            "expected Dexterity source on Evasion Increased step; got {:?}",
            inc.sources
        );
    }

    /// Issue #34 follow-up: Ward routes through the same `pool_basic`
    /// helper but most builds have zero Ward. Verify the breakdown
    /// still returns sensibly with no mods at all — Base step value
    /// is 0 and the Ward final step is 0.
    #[test]
    fn ward_breakdown_returns_zero_baseline() {
        let env = env_with_output();
        let bd = derive_for(&env, "Ward").unwrap();
        assert_eq!(bd.total, 0.0);
        let base = bd.steps.iter().find(|s| s.label == "Base").unwrap();
        assert_eq!(base.value, Some(0.0));
    }

    /// Issue #34 follow-up: LifeRegen walks Flat → Percent → Final.
    /// Flat regen mods land in the Flat step's source list; the
    /// percent step's value shows the amount the percent contributes
    /// (life × pct / 100), not the percent itself.
    #[test]
    fn life_regen_breakdown_flat_plus_percent() {
        let env = env_with_output();
        let bd = derive_for(&env, "LifeRegen").unwrap();
        let labels: Vec<&str> = bd.steps.iter().map(|s| s.label.as_str()).collect();
        assert!(labels.contains(&"Flat regen"));
        assert!(labels.contains(&"Percent regen"));
        assert!(labels.contains(&"Life regen"));

        let flat = bd.steps.iter().find(|s| s.label == "Flat regen").unwrap();
        assert_eq!(flat.value, Some(50.0));
        assert!(
            flat.sources
                .iter()
                .any(|s| s.source.contains("item slot 8")),
            "expected item slot 8 source on flat regen step; got {:?}",
            flat.sources
        );

        let pct = bd
            .steps
            .iter()
            .find(|s| s.label == "Percent regen")
            .unwrap();
        // 4% of 1100 Life = 44.0
        assert!((pct.value.unwrap() - 44.0).abs() < 1e-6);
        assert!(
            pct.sources.iter().any(|s| s.source == "tree"),
            "expected tree source on percent regen step; got {:?}",
            pct.sources
        );

        assert_eq!(bd.total, 94.0);
    }

    /// Issue #34 follow-up: EnergyShieldRecharge breakdown. ES
    /// recharge is `EnergyShield × 0.33 × (1 + Σ INC(EnergyShieldRecharge) / 100)`.
    /// Baseline-only case: 1000 ES × 0.33 = 330/sec, no INC mods.
    #[test]
    fn es_recharge_breakdown_baseline_only() {
        let mut env = Env::default();
        env.output.set("EnergyShield", 1000.0);
        env.output.set("EnergyShieldRecharge", 330.0);
        let bd = derive_for(&env, "EnergyShieldRecharge").unwrap();
        let labels: Vec<&str> = bd.steps.iter().map(|s| s.label.as_str()).collect();
        assert!(labels.contains(&"Baseline"));
        assert!(!labels.contains(&"Increased"));
        assert!(labels.contains(&"Energy shield recharge"));

        let baseline = bd.steps.iter().find(|s| s.label == "Baseline").unwrap();
        // 33% × 1000 = 330.
        assert!((baseline.value.unwrap() - 330.0).abs() < 1e-6);
        assert!((bd.total - 330.0).abs() < 1e-6);
    }

    /// Issue #34 follow-up: EnergyShieldRecharge with INC mods —
    /// 500 ES × 0.33 × (1 + 50%) = 247.5/sec. Inc step appears with
    /// the 1.5 multiplier value.
    #[test]
    fn es_recharge_breakdown_chains_baseline_and_inc() {
        let mut env = Env::default();
        env.output.set("EnergyShield", 500.0);
        env.mod_db
            .add(Mod::inc("EnergyShieldRecharge", 50.0).with_source(Source::Tree));
        env.output.set("EnergyShieldRecharge", 247.5);
        let bd = derive_for(&env, "EnergyShieldRecharge").unwrap();
        let labels: Vec<&str> = bd.steps.iter().map(|s| s.label.as_str()).collect();
        assert!(labels.contains(&"Baseline"));
        assert!(labels.contains(&"Increased"));
        assert!(labels.contains(&"Energy shield recharge"));
        assert!(
            (bd.total - 247.5).abs() < 1e-6,
            "expected 247.5/sec (165 × 1.5), got {}",
            bd.total
        );
    }

    /// Issue #34 follow-up: EnergyShieldRecharge with no ES pool
    /// returns None — there's nothing for the breakdown to display.
    #[test]
    fn es_recharge_breakdown_skipped_when_no_pool() {
        let env = Env::default();
        assert!(
            derive_for(&env, "EnergyShieldRecharge").is_none(),
            "expected None when EnergyShield is zero",
        );
    }

    /// Issue #34 follow-up: EnergyShieldRegen breakdown. ES regen
    /// has no baseline pool % term — it's `flat × (1 + inc/100)`.
    /// With no flat or inc mods the breakdown is empty (the
    /// dispatch returns `None`).
    #[test]
    fn es_regen_breakdown_skipped_when_no_mods() {
        let env = Env::default();
        assert!(
            derive_for(&env, "EnergyShieldRegen").is_none(),
            "expected None when no EnergyShieldRegen mods are set",
        );
    }

    /// Issue #34 follow-up: EnergyShieldRegen with flat-only mods —
    /// 8/sec from an item slot lands as the Flat regen step + Final.
    /// Builds a fresh env so the fixture's representative regen mods
    /// (added for the COVERED_KEYS guard) don't bleed in.
    #[test]
    fn es_regen_breakdown_flat_only() {
        let mut env = Env::default();
        env.mod_db
            .add(Mod::base("EnergyShieldRegen", 8.0).with_source(Source::Item(8)));
        env.output.set("EnergyShieldRegen", 8.0);
        let bd = derive_for(&env, "EnergyShieldRegen").unwrap();
        let labels: Vec<&str> = bd.steps.iter().map(|s| s.label.as_str()).collect();
        assert!(labels.contains(&"Flat regen"));
        assert!(!labels.contains(&"Increased"));
        assert!(labels.contains(&"Energy shield regen"));

        let flat = bd.steps.iter().find(|s| s.label == "Flat regen").unwrap();
        assert_eq!(flat.value, Some(8.0));
        assert!(
            flat.sources
                .iter()
                .any(|s| s.source.contains("item slot 8")),
            "expected item slot 8 source on flat regen step; got {:?}",
            flat.sources
        );
        assert!((bd.total - 8.0).abs() < 1e-6);
    }

    /// Issue #34 follow-up: EnergyShieldRegen chains Flat → Inc →
    /// Final. With +5/sec flat from an item and +50% INC from the
    /// tree the total lands at 5 × 1.5 = 7.5/sec.
    #[test]
    fn es_regen_breakdown_chains_flat_and_inc() {
        let mut env = Env::default();
        env.mod_db
            .add(Mod::base("EnergyShieldRegen", 5.0).with_source(Source::Item(7)));
        env.mod_db
            .add(Mod::inc("EnergyShieldRegen", 50.0).with_source(Source::Tree));
        env.output.set("EnergyShieldRegen", 7.5);
        let bd = derive_for(&env, "EnergyShieldRegen").unwrap();
        let labels: Vec<&str> = bd.steps.iter().map(|s| s.label.as_str()).collect();
        assert!(labels.contains(&"Flat regen"));
        assert!(labels.contains(&"Increased"));
        assert!(labels.contains(&"Energy shield regen"));
        assert!(
            (bd.total - 7.5).abs() < 1e-6,
            "expected 7.5/sec (5 × 1.5), got {}",
            bd.total
        );
    }

    /// Issue #34 follow-up: ManaRegen walks Baseline → Final. With
    /// no extra mods the only step beyond baseline is the final
    /// Mana regen line.
    #[test]
    fn mana_regen_breakdown_baseline_only() {
        let env = env_with_output();
        let bd = derive_for(&env, "ManaRegen").unwrap();
        let labels: Vec<&str> = bd.steps.iter().map(|s| s.label.as_str()).collect();
        assert!(labels.contains(&"Baseline"));
        assert!(labels.contains(&"Mana regen"));
        // No flat / inc mods on ManaRegen in env_with_output — those
        // steps should be skipped.
        assert!(!labels.contains(&"Flat regen"));
        assert!(!labels.contains(&"Increased"));

        let baseline = bd.steps.iter().find(|s| s.label == "Baseline").unwrap();
        // 1.75% of 360 Mana = 6.3.
        assert!((baseline.value.unwrap() - 6.3).abs() < 1e-9);
    }

    /// Issue #34 follow-up: ManaRegen walks Baseline → Flat → Increased
    /// → Final when both adders are present. Verify the chain stays
    /// stable when an item adds 5/sec flat and a tree adds 50% INC.
    #[test]
    fn mana_regen_breakdown_chains_flat_and_inc() {
        let mut env = env_with_output();
        env.mod_db
            .add(Mod::base("ManaRegen", 5.0).with_source(Source::Item(7)));
        env.mod_db
            .add(Mod::inc("ManaRegen", 50.0).with_source(Source::Tree));
        // (6.3 + 5.0) × 1.5 = 16.95
        env.output.set("ManaRegen", 16.95);
        let bd = derive_for(&env, "ManaRegen").unwrap();
        let labels: Vec<&str> = bd.steps.iter().map(|s| s.label.as_str()).collect();
        assert!(labels.contains(&"Baseline"));
        assert!(labels.contains(&"Flat regen"));
        assert!(labels.contains(&"Increased"));
        assert!(labels.contains(&"Mana regen"));
        assert!((bd.total - 16.95).abs() < 1e-6);
    }

    /// Issue #34 follow-up: CastRate is set in `perform_skill_dps`
    /// (perform.rs ~line 4320) as `env.output.set("CastRate", cps)`,
    /// where `cps` is the same value already exposed as
    /// `MainSkillSpeed`. Surfacing the alias as a single-step
    /// breakdown keeps the Calcs panel click-through chain
    /// consistent for skills that use cast-time terminology.
    #[test]
    fn cast_rate_breakdown_aliases_main_skill_speed() {
        let mut env = Env::default();
        env.output.set("MainSkillSpeed", 5.0);
        env.output.set("CastRate", 5.0);
        let bd = derive_for(&env, "CastRate").unwrap();
        let labels: Vec<&str> = bd.steps.iter().map(|s| s.label.as_str()).collect();
        assert!(labels.contains(&"Cast rate"));

        let final_step = bd.steps.last().unwrap();
        assert!(
            final_step
                .explain
                .as_deref()
                .is_some_and(|e| e.contains("MainSkillSpeed") || e.contains("alias")),
            "expected MainSkillSpeed alias hint in explain, got {:?}",
            final_step.explain
        );
        assert!((bd.total - 5.0).abs() < 1e-9);
    }

    /// Issue #34 follow-up: returns None when no skill is loaded
    /// (CastRate = 0).
    #[test]
    fn cast_rate_breakdown_skipped_when_no_skill() {
        let env = Env::default();
        assert!(
            derive_for(&env, "CastRate").is_none(),
            "expected None when CastRate is zero",
        );
    }
}
