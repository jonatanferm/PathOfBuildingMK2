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

// --- Helpers --------------------------------------------------------

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
}
