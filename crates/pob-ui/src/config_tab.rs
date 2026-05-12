//! Config tab — enemy state + condition / multiplier toggles.
//!
//! Key names match what `pob_engine::mod_parser` emits, so toggling a checkbox
//! actually activates `Tag::condition(...)`-tagged mods at perform time.
//! Reference: PoB's `src/Modules/ConfigOptions.lua` (canonical option list) +
//! our `crates/pob-engine/src/mod_parser.rs` (`match_while_var`,
//! `recent_event_var`, `strip_if_havent_clause`, "Nearby Enemies are X").

use eframe::egui;
use pob_engine::character::{ConfigState, EnemyBoss};

/// Issue #19 follow-up: convert `config.exerted_attack_uptime` (0.0..=1.0)
/// to a whole-percent value for the slider widget. Clamps out-of-range
/// inputs defensively in case an old `.mk2` file slipped past sanitise.
#[must_use]
pub fn exerted_uptime_to_percent(uptime: f64) -> i32 {
    (uptime.clamp(0.0, 1.0) * 100.0).round() as i32
}

/// Config tab: how many of the player-side condition toggles are
/// currently active. PoB's `ConfigOptions.lua` ships a fairly long
/// catalogue and a build can accumulate stale toggles from earlier
/// what-if explorations; the header-row chip surfaces "Active: N" so
/// users can tell at a glance whether they left anything on.
///
/// The storage convention (set elsewhere in this file) is "absent or
/// `false` = off" — checking off in the UI removes the key entirely
/// rather than writing `false`. We mirror that here by counting only
/// entries whose value is `true`, so a recovered save that happened to
/// pin a `false` doesn't bias the count.
///
/// Pure / no-egui so the rule stays unit-testable in isolation.
#[must_use]
pub fn count_active_conditions(state: &ConfigState) -> usize {
    state.conditions.values().filter(|v| **v).count()
}

/// Config tab: reset every multiplier (charges, rage, fortification
/// stacks, …) back to its built-in default by clearing
/// `state.multipliers` entirely. Each slider in
/// [`MULTIPLIERS`] falls back to its hard-coded default via
/// `state.multipliers.get(key).copied().unwrap_or(default)`, so the
/// cleanest reset is "drop the override map and let the sliders
/// re-seed from `default` on the next frame".
///
/// Returns `true` iff at least one entry was removed — the caller
/// uses that to gate a recompute so a no-op reset click on a cold-
/// open Config tab doesn't churn the engine.
///
/// Pure / no-egui so the rule is documented and unit-testable in
/// isolation. Symmetric to [`clear_active_conditions`].
pub fn reset_multipliers_to_defaults(state: &mut ConfigState) -> bool {
    let before = state.multipliers.len();
    state.multipliers.clear();
    before > 0
}

/// Config tab: turn off every player-side condition by removing every
/// truthy entry from `state.conditions`. Mirrors what the UI does when
/// a checkbox is toggled off (`state.conditions.remove(key)`), so a
/// subsequent serialise round-trip emits the same shape as if the user
/// had clicked off each box individually.
///
/// Returns `true` iff at least one entry was removed — the caller uses
/// that to gate a recompute so a no-op "Clear" click on an already-
/// empty conditions map doesn't churn the engine.
pub fn clear_active_conditions(state: &mut ConfigState) -> bool {
    let before = state.conditions.len();
    state.conditions.retain(|_, v| !*v);
    state.conditions.len() != before
}

/// Inverse of [`exerted_uptime_to_percent`] — convert a slider %
/// reading back into the 0.0..=1.0 fraction the engine consumes.
/// Clamps out-of-range inputs so a stale state can't push the engine
/// past 100%.
#[must_use]
pub fn percent_to_exerted_uptime(percent: i32) -> f64 {
    f64::from(percent.clamp(0, 100)) / 100.0
}

/// Groups of `(key, label)` condition checkboxes. Group title is the section
/// label; each item flips `config.conditions[key]`.
const GROUPS: &[(&str, &[(&str, &str)])] = &[
    (
        "Action / status buffs",
        &[
            // Names cribbed from mod_parser::match_while_var (HasOnslaught,
            // HasTailwind, HasAdrenaline, HasArcaneSurge, Fortified, Phasing).
            ("HasOnslaught", "Onslaught"),
            ("HasTailwind", "Tailwind"),
            ("HasAdrenaline", "Adrenaline"),
            ("HasArcaneSurge", "Arcane Surge"),
            ("Phasing", "Phasing"),
            ("Fortified", "Fortified"),
            ("AffectedByHerald", "Affected by a Herald"),
            ("AffectedByAura", "Affected by an Aura"),
            ("UsingFlask", "Flask Effect Active"),
            ("UsingTincture", "Tincture Active"),
            ("Focused", "Focused"),
            ("HasMark", "Has a Mark Active"),
            // Player-side debuffs / hits taken (cribbed from match_while_var).
            ("Bleeding", "Bleeding"),
            ("Ignited", "Ignited"),
            ("Frozen", "Frozen"),
            ("Shocked", "Shocked"),
            ("Chilled", "Chilled"),
            ("Cursed", "Cursed"),
        ],
    ),
    (
        "Recently",
        &[
            // From mod_parser::recent_event_var (event + "Recently") and
            // strip_if_havent_clause (Been*Recently). PoB ConfigOptions.lua
            // uses the same names: KilledRecently, HitRecently, CritRecently,
            // BlockedRecently, StunnedRecently, BeenHitRecently, etc.
            ("KilledRecently", "Killed an Enemy Recently"),
            ("HitRecently", "Hit Recently"),
            ("CritRecently", "Crit Recently"),
            ("CastSpellRecently", "Cast a Spell Recently"),
            ("UsedSkillRecently", "Used a Skill Recently"),
            // Issue #19 (slice 5): Warcry usage conditions. Mirrors PoB's
            // ConfigOptions.lua:1528-1534 — gates "while you've used a
            // warcry" mods (Berserker ascendancy, certain notables) plus
            // the 8-second window variant.
            ("UsedWarcryRecently", "Used a Warcry Recently"),
            ("UsedWarcryInPast8Seconds", "Used a Warcry in the past 8s"),
            (
                "WarcryMaxHit",
                "Show max hit instead of average (warcry uptime = 100%)",
            ),
            ("BlockedRecently", "Blocked Recently"),
            ("StunnedEnemyRecently", "Stunned an Enemy Recently"),
            ("ConsumedCorpseRecently", "Consumed a Corpse Recently"),
            ("BeenHitRecently", "Been Hit Recently"),
            ("BeenCritHitRecently", "Been Critically Hit Recently"),
            ("BeenStunnedRecently", "Been Stunned Recently"),
            ("DamagedRecently", "Damaged Recently"),
            // Element-specific damage taken — PoB ConfigOptions.lua names
            // (HitByFireDamageRecently / HitByColdDamageRecently /
            // HitByLightningDamageRecently / HitBySpellDamageRecently).
            ("HitByFireDamageRecently", "Hit by Fire Damage Recently"),
            ("HitByColdDamageRecently", "Hit by Cold Damage Recently"),
            (
                "HitByLightningDamageRecently",
                "Hit by Lightning Damage Recently",
            ),
            ("HitBySpellDamageRecently", "Hit by Spell Damage Recently"),
        ],
    ),
    (
        "Life / mana state",
        &[
            // From mod_parser::match_while_var. PoB ConfigOptions.lua uses
            // FullLife / LowLife / FullMana / LowMana / FullEnergyShield /
            // LowEnergyShield / HaveEnergyShield as the ifCond names.
            ("FullLife", "At Full Life"),
            ("LowLife", "At Low Life"),
            ("FullMana", "At Full Mana"),
            ("LowMana", "At Low Mana"),
            ("FullEnergyShield", "At Full Energy Shield"),
            ("LowEnergyShield", "At Low Energy Shield"),
            ("HasEnergyShield", "Have Energy Shield"),
            ("Leeching", "Leeching"),
            ("LeechingEnergyShield", "Leeching Energy Shield"),
            ("LeechingMana", "Leeching Mana"),
            ("Stationary", "Stationary"),
            ("Moving", "Moving"),
        ],
    ),
    (
        "Charge state",
        &[
            // PoB normally derives "at max charges" via StatThreshold tags, but
            // the ConfigOptions.lua "minionsConditionFullEnergyShield"-style
            // checkboxes show users expect a boolean. We expose the boolean
            // form so a future parser pass that emits Condition tags for
            // "while at maximum X charges" lights them up. Names follow
            // PoB's "AtMax<X>Charges" convention used elsewhere.
            ("AtMaxFrenzyCharges", "At Maximum Frenzy Charges"),
            ("AtMaxPowerCharges", "At Maximum Power Charges"),
            ("AtMaxEnduranceCharges", "At Maximum Endurance Charges"),
        ],
    ),
    (
        "Combat stance",
        &[
            // From mod_parser::match_while_var.
            ("Channelling", "Channelling"),
            ("Casting", "Casting"),
            ("DualWielding", "Dual Wielding"),
            ("UsingTwoHandedWeapon", "Using Two-Handed Weapon"),
            ("UsingShield", "Using a Shield"),
        ],
    ),
    (
        "Enemy state",
        &[
            // Names match mod_parser's "Nearby Enemies are X" output flags
            // (EnemyShocked, EnemyChilled, EnemyFrozen, EnemyIgnited,
            // EnemyBleeding, EnemyPoisoned) plus EnemyCursed for symmetry.
            // Today few mods gate on these as Condition tags, but this is
            // the canonical key namespace in our engine — saved files round
            // trip and future parser improvements will pick them up.
            ("EnemyShocked", "Enemy is Shocked"),
            ("EnemyChilled", "Enemy is Chilled"),
            ("EnemyFrozen", "Enemy is Frozen"),
            ("EnemyIgnited", "Enemy is Ignited"),
            ("EnemyBleeding", "Enemy is Bleeding"),
            ("EnemyPoisoned", "Enemy is Poisoned"),
            ("EnemyCursed", "Enemy is Cursed"),
            ("EnemyMaimed", "Enemy is Maimed"),
            ("EnemyHindered", "Enemy is Hindered"),
            ("EnemyIntimidated", "Enemy is Intimidated"),
            ("EnemyBlinded", "Enemy is Blinded"),
            ("EnemyUnnerved", "Enemy is Unnerved"),
            ("EnemyCrushed", "Enemy is Crushed"),
            ("EnemyIsBoss", "Enemy is a Boss"),
            // Movement toggle doubles BleedDPS — PoB models bleed-while-moving as a
            // 100% MORE multiplier on bleed damage gated on this condition.
            ("EnemyMoving", "Enemy is Moving"),
        ],
    ),
];

const MULTIPLIERS: &[(&str, &str, f64, f64)] = &[
    // (key, label, default, max). Charges default to 3 to match PoB's
    // newly-rolled character starting state.
    ("PowerCharge", "Power Charges", 3.0, 25.0),
    ("FrenzyCharge", "Frenzy Charges", 3.0, 25.0),
    ("EnduranceCharge", "Endurance Charges", 3.0, 25.0),
    ("Rage", "Rage", 0.0, 100.0),
    ("FortificationStacks", "Fortification Stacks", 0.0, 50.0),
];

pub fn ui(ui: &mut egui::Ui, state: &mut ConfigState) -> bool {
    let mut changed = false;

    ui.horizontal(|ui| {
        ui.vertical(|ui| {
            ui.set_min_width(220.0);
            ui.heading("Enemy");
            ui.separator();
            // Issue #35: Boss preset dropdown. Selecting a preset writes
            // PoB's canonical resist defaults into the resist sliders so
            // the user sees what's about to apply; the engine handles
            // RareOrUnique / PinnacleBoss conditions and AilmentThreshold
            // MORE in init_env_with_bases. Switching to None keeps
            // current resists (the user might want to compare).
            let boss_options: &[(EnemyBoss, &str)] = &[
                (EnemyBoss::None, "No boss preset"),
                (EnemyBoss::Boss, "Standard Boss"),
                (EnemyBoss::Pinnacle, "Pinnacle Boss"),
                (EnemyBoss::Uber, "Uber Pinnacle Boss"),
            ];
            let current_boss_label = boss_options
                .iter()
                .find(|(b, _)| *b == state.enemy_boss)
                .map(|(_, l)| *l)
                .unwrap_or("No boss preset");
            egui::ComboBox::from_label("Is the enemy a boss?")
                .selected_text(current_boss_label)
                .show_ui(ui, |ui| {
                    for (option, label) in boss_options {
                        if ui
                            .selectable_label(state.enemy_boss == *option, *label)
                            .clicked()
                            && state.enemy_boss != *option
                        {
                            state.enemy_boss = *option;
                            // Push canonical defaults for non-None presets:
                            // resists, armour, evasion. Mirrors PoB's
                            // "set placeholders" behaviour — explicit
                            // slider moves still override afterwards.
                            // Pen is engine-side (default_penetration) so
                            // it lands at compute time, not as a slider.
                            if *option != EnemyBoss::None {
                                let (fr, cr, lr, ch) = option.default_resists();
                                state.enemy_fire_resist = fr;
                                state.enemy_cold_resist = cr;
                                state.enemy_lightning_resist = lr;
                                state.enemy_chaos_resist = ch;
                                state.enemy_armour = option.default_armour();
                                state.enemy_evasion = option.default_evasion();
                            }
                            changed = true;
                        }
                    }
                });
            let mut lvl = state.enemy_level as i32;
            if ui
                .add(egui::Slider::new(&mut lvl, 1..=100).text("Enemy level"))
                .changed()
            {
                state.enemy_level = lvl.max(0) as u32;
                changed = true;
            }
            for (label, accessor) in [
                ("Fire resist (%)", &mut state.enemy_fire_resist),
                ("Cold resist (%)", &mut state.enemy_cold_resist),
                ("Lightning resist (%)", &mut state.enemy_lightning_resist),
                ("Chaos resist (%)", &mut state.enemy_chaos_resist),
            ] {
                if ui
                    .add(egui::Slider::new(accessor, -100..=90).text(label))
                    .changed()
                {
                    changed = true;
                }
            }
            let mut ev = state.enemy_evasion as i32;
            if ui
                .add(egui::Slider::new(&mut ev, 0..=20000).text("Enemy evasion"))
                .changed()
            {
                state.enemy_evasion = ev.max(0) as u32;
                changed = true;
            }
            let mut ar = state.enemy_armour as i32;
            if ui
                .add(egui::Slider::new(&mut ar, 0..=50000).text("Enemy armour"))
                .changed()
            {
                state.enemy_armour = ar.max(0) as u32;
                changed = true;
            }
            let mut block = state.enemy_block_chance as i32;
            if ui
                .add(egui::Slider::new(&mut block, 0..=75).text("Enemy block (%)"))
                .changed()
            {
                state.enemy_block_chance = block.max(0) as u32;
                changed = true;
            }
            let mut dodge = state.enemy_dodge_chance as i32;
            if ui
                .add(egui::Slider::new(&mut dodge, 0..=75).text("Enemy dodge (%)"))
                .changed()
            {
                state.enemy_dodge_chance = dodge.max(0) as u32;
                changed = true;
            }
            let mut sup = state.enemy_suppression_chance as i32;
            if ui
                .add(egui::Slider::new(&mut sup, 0..=100).text("Enemy spell suppression (%)"))
                .changed()
            {
                state.enemy_suppression_chance = sup.max(0) as u32;
                changed = true;
            }
            let mut proj = state.projectiles_hitting_target as i32;
            if ui
                .add(egui::Slider::new(&mut proj, 0..=20).text("Projectiles hit target"))
                .changed()
            {
                state.projectiles_hitting_target = proj.max(0) as u32;
                changed = true;
            }
            // Issue #60: AoE shotgun-overlap multiplier. PoB exposes
            // this for skills like Earthquake / Tectonic Slam where
            // overlapping AoE hits stack on a single target.
            let mut aoe = state.enemies_hit_by_aoe as i32;
            if ui
                .add(egui::Slider::new(&mut aoe, 1..=10).text("Enemies hit by AoE"))
                .changed()
            {
                state.enemies_hit_by_aoe = aoe.max(1) as u32;
                changed = true;
            }
            // Issue #83 (slice 2): "# of nearby Enemies" feeds
            // Multiplier:NearbyEnemies + (when ==1) the
            // OnlyOneNearbyEnemy condition; mirrors PoB's
            // `multiplierNearbyEnemies` Config-tab input.
            let mut nearby = state.nearby_enemies as i32;
            if ui
                .add(egui::Slider::new(&mut nearby, 0..=20).text("# of nearby Enemies"))
                .changed()
            {
                state.nearby_enemies = nearby.max(0) as u32;
                changed = true;
            }
            // Issue #19 (slice 15): "# of nearby Allies" feeds
            // Multiplier:NearbyAlly. Drives Rallying Cry's
            // per-ally exert damage bonus, banner skill ally
            // scaling, and party-build "+X% per ally" mods. PoB
            // defaults to 0 (solo).
            let mut nearby_allies = state.nearby_allies as i32;
            if ui
                .add(egui::Slider::new(&mut nearby_allies, 0..=10).text("# of nearby Allies"))
                .changed()
            {
                state.nearby_allies = nearby_allies.max(0) as u32;
                changed = true;
            }
        });

        ui.separator();

        ui.vertical(|ui| {
            ui.set_min_width(300.0);
            ui.horizontal(|ui| {
                ui.heading("Conditions");
                let active = count_active_conditions(state);
                if active > 0 {
                    ui.weak(format!("Active: {active}"));
                    if ui
                        .small_button("Clear")
                        .on_hover_text(
                            "Turn off every player-side condition toggle. \
                             Stale toggles from earlier what-if runs can \
                             linger across builds — this resets them in \
                             one click.",
                        )
                        .clicked()
                        && clear_active_conditions(state)
                    {
                        changed = true;
                    }
                }
            });
            ui.separator();
            egui::ScrollArea::vertical()
                .id_salt("conditions")
                .max_height(520.0)
                .show(ui, |ui| {
                    for (group_label, items) in GROUPS {
                        // First group expanded by default so users see content
                        // immediately; the rest collapsed to keep things compact.
                        let default_open = *group_label == "Action / status buffs";
                        egui::CollapsingHeader::new(*group_label)
                            .default_open(default_open)
                            .id_salt(*group_label)
                            .show(ui, |ui| {
                                for (key, label) in *items {
                                    let mut on =
                                        state.conditions.get(*key).copied().unwrap_or(false);
                                    if ui.checkbox(&mut on, *label).changed() {
                                        if on {
                                            state.conditions.insert((*key).to_owned(), true);
                                        } else {
                                            state.conditions.remove(*key);
                                        }
                                        changed = true;
                                    }
                                }
                            });
                    }
                });
        });

        ui.separator();

        ui.vertical(|ui| {
            ui.set_min_width(220.0);
            ui.horizontal(|ui| {
                ui.heading("Multipliers");
                // Mirror the Conditions panel: surface a Reset button
                // only when there's something to reset, so the cold-
                // open header stays tidy.
                if !state.multipliers.is_empty()
                    && ui
                        .small_button("Reset")
                        .on_hover_text(
                            "Drop every overridden multiplier and let each \
                             slider re-seed from its built-in default.",
                        )
                        .clicked()
                    && reset_multipliers_to_defaults(state)
                {
                    changed = true;
                }
            });
            ui.separator();
            for (key, label, default, max) in MULTIPLIERS {
                let mut v = state.multipliers.get(*key).copied().unwrap_or(*default);
                if ui
                    .add(egui::Slider::new(&mut v, 0.0..=*max).text(*label))
                    .changed()
                {
                    state.multipliers.insert((*key).to_owned(), v);
                    changed = true;
                }
            }
        });
    });

    // Issue #109 (slice 3): swap-weapon set toggle. Mirrors PoB's
    // X-key swap from `Classes/ItemsTab.lua`. When checked the calc
    // engine reads `Weapon1Swap` / `Weapon2Swap` instead of the
    // primary pair via `effective_items_for_compute`. Empty swap
    // slots fall through to the primary pair (no-op), so flipping
    // the toggle on a single-pair build is harmless.
    ui.separator();
    if ui
        .checkbox(&mut state.use_second_weapon_set, "Use swap weapon set")
        .on_hover_text(
            "When checked, the calc engine reads the swap-pair weapons \
             (Weapon1Swap / Weapon2Swap) as the live pair. Useful for \
             caster off-hand-buff stacking + Storm Brand swap-trap builds.",
        )
        .changed()
    {
        changed = true;
    }

    // Issue #19 (slice 2): Warcry Power config knob. Mirrors PoB's
    // `multiplierWarcryPower` Config-tab input from
    // `Modules/ConfigOptions.lua:723-725`. Power is the strength of
    // nearby enemies summed up (1 normal, 2 magic, 10 rare, 20
    // unique); PoB's tooltip suggests 20 (one boss) as a default.
    ui.separator();
    ui.horizontal(|ui| {
        ui.label("Warcry Power:");
        let mut enabled = state.warcry_power.is_some();
        if ui.checkbox(&mut enabled, "").changed() {
            state.warcry_power = if enabled { Some(20) } else { None };
            changed = true;
        }
        if let Some(power) = state.warcry_power.as_mut() {
            let mut as_i32 = *power as i32;
            if ui
                .add(egui::DragValue::new(&mut as_i32).range(0..=999))
                .changed()
            {
                let clamped = as_i32.clamp(0, 999) as u32;
                if clamped != *power {
                    *power = clamped;
                    changed = true;
                }
            }
        } else {
            ui.weak("(disabled — defaults to PoB's 20-power boss assumption)");
        }
    });

    // Issue #19 follow-up: Exerted Attack Uptime slider. PoB's
    // `Modules/ConfigOptions.lua` exposes this as a numeric input;
    // the engine reads it at `perform.rs:4026` to override the
    // auto-derived value (which is still empty for most skills). 0%
    // means "no warcry-exerted hits" (the engine default); 100% means
    // "every hit is exerted".
    ui.horizontal(|ui| {
        ui.label("Exerted Attack Uptime:");
        let mut percent = exerted_uptime_to_percent(state.exerted_attack_uptime);
        if ui
            .add(egui::Slider::new(&mut percent, 0..=100).suffix("%"))
            .on_hover_text(
                "Fraction of the player's attacks that are exerted by an active \
                 warcry. Each exerted hit picks up the `ExertedAttackDamage` MORE \
                 bonus from warcry support gems. Leave at 0% if no warcry is being \
                 cast for the encounter.",
            )
            .changed()
        {
            let next = percent_to_exerted_uptime(percent);
            if (next - state.exerted_attack_uptime).abs() > f64::EPSILON {
                state.exerted_attack_uptime = next;
                changed = true;
            }
        }
    });

    // Issue #28: Custom Modifiers textarea. Mirrors PoB's Config-tab
    // free-form mod input — each non-empty line is parsed by `mod_parser`
    // and added to the player modDB with `source = Custom`. The engine
    // half landed in PR #63; this is the UI surface.
    ui.separator();
    ui.heading("Custom Modifiers");
    ui.label(
        "One PoB-style mod line per row (e.g. `+50 to Strength`, `100% increased Fire Damage`). \
         Unparseable lines are highlighted below — fix or remove them so the engine can apply \
         the rest.",
    );
    let response = ui.add(
        egui::TextEdit::multiline(&mut state.custom_mods)
            .desired_width(f32::INFINITY)
            .desired_rows(6)
            .hint_text("Custom mods, one per line — used for what-if testing.")
            .font(egui::TextStyle::Monospace),
    );
    if response.changed() {
        changed = true;
    }
    // Surface a quick parse-status summary so users can spot bad lines
    // without leaving the tab. Each line is checked through the same
    // `parse_mod_line` the engine uses at perform time. Issue #28
    // closes the "inline parse error" acceptance criterion: when at
    // least one line fails we list the first few offending lines
    // (with line numbers) so the user can fix them in place instead
    // of guessing which row is the problem.
    let mut total = 0usize;
    let mut failing: Vec<(usize, String)> = Vec::new();
    for (idx, raw_line) in state.custom_mods.lines().enumerate() {
        let trimmed = raw_line.trim();
        if trimmed.is_empty() {
            continue;
        }
        total += 1;
        if pob_engine::mod_parser::parse_mod_line(trimmed).is_none() {
            failing.push((idx + 1, trimmed.to_owned()));
        }
    }
    if total > 0 {
        let parsed = total - failing.len();
        let color = if failing.is_empty() {
            egui::Color32::from_rgb(0x33, 0xFF, 0x77)
        } else {
            egui::Color32::from_rgb(0xFF, 0x99, 0x22)
        };
        ui.colored_label(color, format!("{parsed} / {total} lines parse"));
        // List up to 3 failing lines verbatim — enough to guide the
        // user without flooding the panel when they paste a 50-line
        // chunk of garbage. The remainder collapses to a "+N more"
        // hint.
        const MAX_FAILED_SHOWN: usize = 3;
        for (line_no, body) in failing.iter().take(MAX_FAILED_SHOWN) {
            ui.colored_label(
                egui::Color32::from_rgb(0xFF, 0x99, 0x22),
                format!("  L{line_no}: {body}"),
            );
        }
        if failing.len() > MAX_FAILED_SHOWN {
            ui.weak(format!(
                "  …+{} more failing line{}",
                failing.len() - MAX_FAILED_SHOWN,
                if failing.len() - MAX_FAILED_SHOWN == 1 {
                    ""
                } else {
                    "s"
                }
            ));
        }
    }

    changed
}

#[cfg(test)]
mod tests {
    use super::*;
    use pob_engine::mod_db::eval_mod;
    use pob_engine::mod_db::EvalState;
    use pob_engine::modifier::{Mod, Tag};

    #[test]
    fn exerted_uptime_round_trips_at_integer_percents() {
        // Walking every integer percent through the conversion and back
        // should land on the same percent — the slider clicks at
        // integer steps so this is the user-visible identity.
        for p in 0..=100 {
            let frac = percent_to_exerted_uptime(p);
            let back = exerted_uptime_to_percent(frac);
            assert_eq!(back, p, "round-trip failed at p={p}, frac={frac}");
        }
    }

    #[test]
    fn exerted_uptime_to_percent_clamps_out_of_range() {
        // A stale `.mk2` file could carry an out-of-band fraction
        // (engine reads `clamp(0.0, 1.0)` anyway); the slider conv
        // mirrors that contract so the widget reads sanely.
        assert_eq!(exerted_uptime_to_percent(-0.5), 0);
        assert_eq!(exerted_uptime_to_percent(2.0), 100);
    }

    #[test]
    fn percent_to_exerted_uptime_clamps_out_of_range() {
        // Defensive against a stale slider state — the engine
        // recomputes its own clamp, but pinning the UI side avoids
        // pushing a "150%" through the pipeline at all.
        assert!((percent_to_exerted_uptime(-10) - 0.0).abs() < f64::EPSILON);
        assert!((percent_to_exerted_uptime(150) - 1.0).abs() < f64::EPSILON);
    }

    #[test]
    fn exerted_uptime_to_percent_rounds_to_nearest_percent() {
        // PoB exposes whole-percent steps; the conversion picks the
        // nearest integer so a fraction at exactly 0.505 doesn't
        // truncate to 50%.
        assert_eq!(exerted_uptime_to_percent(0.505), 51);
        assert_eq!(exerted_uptime_to_percent(0.504), 50);
    }

    /// Sanity: every key the UI presents should at least parse as a Rust
    /// identifier-like ASCII string, so persistence to PoB-XML round-trips.
    #[test]
    fn condition_keys_are_simple_ascii() {
        for (_group, items) in GROUPS {
            for (key, _label) in *items {
                assert!(!key.is_empty());
                assert!(
                    key.chars().all(|c| c.is_ascii_alphanumeric()),
                    "non-alphanumeric key {key:?} would break PoB-XML round trip"
                );
            }
        }
    }

    /// Flipping a UI condition (here `FullLife`) on `EvalState` activates a
    /// `Tag::condition("FullLife")`-gated mod. This is the same gate the
    /// perform pass applies — see `perform.rs` step 6 ("Config — push
    /// conditions and multipliers into the eval state").
    #[test]
    fn ui_keys_match_engine_condition_tags() {
        // Spot-check three keys covering each major group.
        for key in ["FullLife", "HasOnslaught", "KilledRecently"] {
            let m = Mod::inc("Life", 30.0).with_tag(Tag::condition(key));

            // Off: condition absent → mod is gated out.
            let off = EvalState::default();
            assert_eq!(
                eval_mod(&m, &off),
                None,
                "{key}: should not evaluate when condition is unset"
            );

            // On: condition true → mod evaluates to its raw value.
            let mut on = EvalState::default();
            on.set_condition(key, true);
            assert_eq!(
                eval_mod(&m, &on),
                Some(30.0),
                "{key}: should evaluate when condition is true"
            );
        }
    }

    /// Toggling on a multiplier key (`PowerCharge`) activates a
    /// `Tag::multiplier("PowerCharge")`-tagged mod by the count.
    #[test]
    fn multiplier_keys_scale_tagged_mods() {
        let m = Mod::base("Damage", 5.0).with_tag(Tag::multiplier("PowerCharge"));

        let zero = EvalState::default();
        assert_eq!(eval_mod(&m, &zero), Some(0.0));

        let mut three = EvalState::default();
        three.set_multiplier("PowerCharge", 3.0);
        assert_eq!(eval_mod(&m, &three), Some(15.0));
    }

    // ─── count_active_conditions / clear_active_conditions ───────────────

    #[test]
    fn count_active_conditions_returns_zero_for_default_state() {
        // Cold-open: no conditions touched. The chip stays hidden.
        let state = ConfigState::default();
        assert_eq!(count_active_conditions(&state), 0);
    }

    #[test]
    fn count_active_conditions_ignores_false_entries() {
        // The UI removes the key when toggling off, so a `false` entry
        // would only arrive from a hand-edited save. The count rule
        // must not be biased by them — match what the user sees in the
        // checkbox grid.
        let mut state = ConfigState::default();
        state.conditions.insert("FullLife".to_owned(), true);
        state.conditions.insert("Stale".to_owned(), false);
        state.conditions.insert("OnFire".to_owned(), true);
        assert_eq!(count_active_conditions(&state), 2);
    }

    #[test]
    fn clear_active_conditions_drops_truthy_entries() {
        // The clear path removes truthy entries entirely (matching the
        // UI's "toggle off = remove" convention) so a serialise round-
        // trip emits the same shape as if the user had unticked each
        // box individually.
        let mut state = ConfigState::default();
        state.conditions.insert("FullLife".to_owned(), true);
        state.conditions.insert("OnFire".to_owned(), true);
        let changed = clear_active_conditions(&mut state);
        assert!(changed);
        assert_eq!(count_active_conditions(&state), 0);
        assert!(
            state.conditions.is_empty(),
            "truthy entries should be removed, not just zeroed"
        );
    }

    #[test]
    fn clear_active_conditions_preserves_false_entries() {
        // A hand-edited `false` is meaningful (it pins a non-default),
        // so the clear path should NOT remove it — only truthy entries
        // are interpreted as "active".
        let mut state = ConfigState::default();
        state.conditions.insert("FullLife".to_owned(), true);
        state.conditions.insert("Pinned".to_owned(), false);
        clear_active_conditions(&mut state);
        assert_eq!(state.conditions.get("Pinned"), Some(&false));
        assert!(state.conditions.get("FullLife").is_none());
    }

    #[test]
    fn clear_active_conditions_no_op_returns_false() {
        // Empty / already-cleared state: the helper must report no
        // change so the caller doesn't trigger a recompute. Mirrors
        // the same gate `set_all_party_members_enabled` uses.
        let mut state = ConfigState::default();
        assert!(!clear_active_conditions(&mut state));
        state.conditions.insert("Pinned".to_owned(), false);
        assert!(
            !clear_active_conditions(&mut state),
            "removing only truthy entries shouldn't fire on a `false`-only map"
        );
    }

    // ─── reset_multipliers_to_defaults ───────────────────────────────────

    #[test]
    fn reset_multipliers_to_defaults_clears_overrides() {
        // Drop any overridden multiplier so each slider re-seeds
        // from its built-in default on the next frame.
        let mut state = ConfigState::default();
        state.multipliers.insert("PowerCharge".to_owned(), 7.0);
        state.multipliers.insert("Rage".to_owned(), 42.0);
        let changed = reset_multipliers_to_defaults(&mut state);
        assert!(changed);
        assert!(state.multipliers.is_empty());
    }

    #[test]
    fn reset_multipliers_to_defaults_no_op_returns_false() {
        // Empty override map: the helper must report no change so the
        // caller can skip the recompute. Same gate the conditions
        // reset uses.
        let mut state = ConfigState::default();
        assert!(!reset_multipliers_to_defaults(&mut state));
    }
}
