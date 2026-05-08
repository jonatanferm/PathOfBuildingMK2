//! Config tab — enemy state + condition / multiplier toggles.
//!
//! Key names match what `pob_engine::mod_parser` emits, so toggling a checkbox
//! actually activates `Tag::condition(...)`-tagged mods at perform time.
//! Reference: PoB's `src/Modules/ConfigOptions.lua` (canonical option list) +
//! our `crates/pob-engine/src/mod_parser.rs` (`match_while_var`,
//! `recent_event_var`, `strip_if_havent_clause`, "Nearby Enemies are X").

use eframe::egui;
use pob_engine::character::ConfigState;

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
        });

        ui.separator();

        ui.vertical(|ui| {
            ui.set_min_width(300.0);
            ui.heading("Conditions");
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
            ui.heading("Multipliers");
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

    changed
}

#[cfg(test)]
mod tests {
    use super::*;
    use pob_engine::mod_db::eval_mod;
    use pob_engine::mod_db::EvalState;
    use pob_engine::modifier::{Mod, Tag};

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
}
