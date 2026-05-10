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
    // Hit chance.
    "Accuracy",
    "MainSkillHitChance",
    // Damage chain.
    "MainSkillAverageHitAfterResist",
    "MainSkillAverageHitAfterShock",
    "MainSkillAverageHitAfterAccuracy",
    // Spell-resource cost.
    "MainSkillManaCost",
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
        env.output.set("CritChance", 6.0);
        env.output.set("FullDPS", 2000.0);
        env.output.set("BleedDPS", 0.0);
        env.output.set("PoisonDPS", 350.0);
        env.output.set("IgniteDPS", 0.0);
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
        // Pool outputs + their attribute drivers so the Life / Mana
        // breakdowns have something to walk. The numbers track a
        // representative L90 character: 1100 Life from 50 base + 12×89
        // class-and-level + 540 from items + 80 Str / 2.
        env.output.set("Life", 1100.0);
        env.output.set("Mana", 360.0);
        env.output.set("Strength", 80.0);
        env.output.set("Dexterity", 50.0);
        env.output.set("Intelligence", 60.0);
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
}
