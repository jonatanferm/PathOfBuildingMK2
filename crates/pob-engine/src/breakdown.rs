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

        _ => None,
    }
}

/// Names of every output key that has a custom breakdown. Useful for
/// integration tests / docs.
pub const COVERED_KEYS: &[&str] = &[
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
                .with_explain(format!(
                    "{default_cap:.0} + {cap_bonus:.0}% from {ck} mods"
                ))
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
        env.output.set("MainSkillAverageHitAfterResist", 280.0);
        env.output.set("MainSkillAverageHitAfterShock", 280.0);
        env.output.set("MainSkillAverageHitAfterAccuracy", 280.0);
        env.output.set("MainSkillEnemyEffectiveResist", 30.0);
        env.output.set("MainSkillHitChance", 100.0);
        env.output.set("GemQuality", 20.0);
        // Pool outputs + their attribute drivers so the Life / Mana
        // breakdowns have something to walk. The numbers track a
        // representative L90 character: 1100 Life from 50 base + 12×89
        // class-and-level + 540 from items + 80 Str / 2.
        env.output.set("Life", 1100.0);
        env.output.set("Mana", 360.0);
        env.output.set("Strength", 80.0);
        env.output.set("Intelligence", 60.0);
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
}
