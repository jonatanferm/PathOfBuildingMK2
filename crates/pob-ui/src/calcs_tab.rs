//! Calcs tab — flat dump of every computed output stat. Phase 5 minimum: a sortable,
//! filterable table view. The proper "click-to-breakdown" panel comes later — it needs
//! the engine to expose the modifier list contributing to a given stat (which we have
//! the data for via ModDB::iter_named, just not surfaced yet).

use eframe::egui;
use pob_engine::Output;

pub struct CalcsTabState {
    pub filter: String,
    pub hide_zero: bool,
}

impl Default for CalcsTabState {
    fn default() -> Self {
        Self {
            filter: String::new(),
            hide_zero: false,
        }
    }
}

/// Stat category groupings — each (heading, prefix-or-substring-list).
const GROUPS: &[(&str, &[&str])] = &[
    ("Attributes", &["Strength", "Dexterity", "Intelligence", "AllAttributes"]),
    ("Pools", &["Life", "Mana", "EnergyShield", "Ward", "Rage"]),
    ("Resists", &["FireResist", "ColdResist", "LightningResist", "ChaosResist", "ElementalResist"]),
    ("Defences", &["Armour", "Evasion", "Block", "Spell", "Suppress", "Recover", "Regen", "Recharge", "Phys"]),
    ("EHP", &["EHP"]),
    ("Charges & Multipliers", &["Charge", "Crit", "Power", "Frenzy", "Endurance"]),
    ("Speeds", &["Speed", "Accuracy"]),
    ("Main Skill", &["MainSkill", "FullDPS"]),
    ("Ailments", &["Bleed", "Poison", "Ignite", "Freeze", "Shock", "Chill", "Ailment"]),
    ("Misc", &["Misc:", "Keystone:"]),
];

pub fn ui(ui: &mut egui::Ui, state: &mut CalcsTabState, output: &Output) {
    ui.horizontal(|ui| {
        ui.label("Filter:");
        ui.add(
            egui::TextEdit::singleline(&mut state.filter)
                .desired_width(220.0)
                .hint_text("Life, FireResist, MainSkill, …"),
        );
        ui.checkbox(&mut state.hide_zero, "Hide zero values");
        ui.separator();
        ui.label(format!("{} stats", output.len()));
    });
    ui.separator();

    let q = state.filter.trim().to_lowercase();
    let mut entries: Vec<(&str, f64)> = output.iter().collect();
    entries.sort_by(|a, b| a.0.cmp(b.0));
    let entries_filtered: Vec<(&str, f64)> = entries
        .into_iter()
        .filter(|(k, _)| q.is_empty() || k.to_lowercase().contains(&q))
        .filter(|(_, v)| !state.hide_zero || v.abs() > 1e-9)
        .collect();

    egui::ScrollArea::vertical()
        .auto_shrink([false, false])
        .show(ui, |ui| {
            // Bucket into groups; track which entries we've shown so the "Other" group
            // catches leftovers.
            let mut shown: std::collections::HashSet<&str> = Default::default();
            for (heading, patterns) in GROUPS {
                let group_entries: Vec<&(&str, f64)> = entries_filtered
                    .iter()
                    .filter(|(k, _)| {
                        patterns
                            .iter()
                            .any(|p| if p.ends_with(':') { k.starts_with(p) } else { k.contains(p) })
                    })
                    .collect();
                if group_entries.is_empty() {
                    continue;
                }
                ui.collapsing(*heading, |ui| {
                    egui::Grid::new(format!("calcs_grid_{heading}"))
                        .num_columns(2)
                        .striped(true)
                        .show(ui, |ui| {
                            for (k, v) in group_entries {
                                shown.insert(*k);
                                render_row(ui, k, *v);
                            }
                        });
                });
            }
            let leftovers: Vec<_> = entries_filtered
                .iter()
                .filter(|(k, _)| !shown.contains(k))
                .collect();
            if !leftovers.is_empty() {
                ui.collapsing("Other", |ui| {
                    egui::Grid::new("calcs_grid_other")
                        .num_columns(2)
                        .striped(true)
                        .show(ui, |ui| {
                            for (k, v) in leftovers {
                                render_row(ui, k, *v);
                            }
                        });
                });
            }
        });
}

fn render_row(ui: &mut egui::Ui, k: &str, v: f64) {
    ui.monospace(k);
    ui.with_layout(egui::Layout::right_to_left(egui::Align::Min), |ui| {
        let formatted = if v.fract().abs() < 1e-9 {
            format!("{v:>12.0}")
        } else if v.abs() < 100.0 {
            format!("{v:>12.4}")
        } else {
            format!("{v:>12.2}")
        };
        ui.monospace(formatted);
    });
    ui.end_row();
}
