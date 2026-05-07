//! Config tab — enemy state + condition / multiplier toggles.

use eframe::egui;
use pob_engine::character::ConfigState;

const CONDITIONS: &[(&str, &str)] = &[
    ("FullLife", "At Full Life"),
    ("LowLife", "At Low Life"),
    ("FullMana", "At Full Mana"),
    ("LowMana", "At Low Mana"),
    ("Leeching", "Leeching"),
    ("Stationary", "Stationary"),
    ("Moving", "Moving"),
    ("Focused", "Focused"),
    ("Phasing", "Phasing"),
    ("Bleeding", "Bleeding"),
    ("Ignited", "Ignited"),
    ("Frozen", "Frozen"),
    ("Shocked", "Shocked"),
    ("Chilled", "Chilled"),
    ("Cursed", "Cursed"),
    ("UsingShield", "Using Shield"),
    ("DualWielding", "Dual Wielding"),
    ("UsingTwoHandedWeapon", "Using Two-Handed Weapon"),
    ("Channelling", "Channelling"),
    ("KilledRecently", "Killed an Enemy Recently"),
    ("CritRecently", "Crit Recently"),
    ("BeenHitRecently", "Been Hit Recently"),
];

const MULTIPLIERS: &[(&str, &str, f64)] = &[
    ("PowerCharge", "Power Charges", 3.0),
    ("FrenzyCharge", "Frenzy Charges", 3.0),
    ("EnduranceCharge", "Endurance Charges", 3.0),
    ("Rage", "Rage", 0.0),
];

pub fn ui(ui: &mut egui::Ui, state: &mut ConfigState) -> bool {
    let mut changed = false;

    ui.horizontal(|ui| {
        ui.vertical(|ui| {
            ui.set_min_width(220.0);
            ui.heading("Enemy");
            ui.separator();
            let mut lvl = state.enemy_level as i32;
            if ui.add(egui::Slider::new(&mut lvl, 1..=100).text("Enemy level")).changed() {
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
        });

        ui.separator();

        ui.vertical(|ui| {
            ui.set_min_width(260.0);
            ui.heading("Conditions");
            ui.separator();
            egui::ScrollArea::vertical()
                .id_salt("conditions")
                .max_height(420.0)
                .show(ui, |ui| {
                    for (key, label) in CONDITIONS {
                        let mut on = state.conditions.get(*key).copied().unwrap_or(false);
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
        });

        ui.separator();

        ui.vertical(|ui| {
            ui.set_min_width(220.0);
            ui.heading("Multipliers");
            ui.separator();
            for (key, label, default) in MULTIPLIERS {
                let mut v = state.multipliers.get(*key).copied().unwrap_or(*default);
                if ui
                    .add(egui::Slider::new(&mut v, 0.0..=100.0).text(*label))
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
