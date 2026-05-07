//! Skills tab — pick the main skill, edit its level + quality.
//!
//! Phase 5 will turn this into a real skill-gem socket editor with support gem links;
//! Phase 3d only needs to drive the engine's `MainSkill`.

use eframe::egui;
use pob_engine::{MainSkill, SkillRegistry};

pub struct SkillsTabState {
    pub filter: String,
    pub show_spells: bool,
    pub show_attacks: bool,
}

impl Default for SkillsTabState {
    fn default() -> Self {
        Self {
            filter: String::new(),
            show_spells: true,
            show_attacks: true,
        }
    }
}

pub fn ui(
    ui: &mut egui::Ui,
    state: &mut SkillsTabState,
    main_skill: &mut Option<MainSkill>,
    registry: &SkillRegistry,
) -> bool {
    let mut changed = false;

    ui.horizontal(|ui| {
        ui.label("Filter:");
        ui.add(
            egui::TextEdit::singleline(&mut state.filter)
                .desired_width(220.0)
                .hint_text("Arc, Fireball, …"),
        );
        ui.checkbox(&mut state.show_spells, "Spells");
        ui.checkbox(&mut state.show_attacks, "Attacks");
        ui.separator();
        ui.label(format!("{} skills loaded", registry.len()));
    });
    ui.separator();

    ui.horizontal(|ui| {
        // Skill list
        ui.vertical(|ui| {
            ui.set_min_width(280.0);
            ui.set_max_width(320.0);
            ui.heading("Active skills");
            egui::ScrollArea::vertical()
                .auto_shrink([false, false])
                .max_height(420.0)
                .show(ui, |ui| {
                    let q = state.filter.trim().to_lowercase();
                    let mut skills: Vec<(&str, &pob_data::Skill)> = registry
                        .iter_active()
                        .filter(|(_, s)| {
                            let is_spell = s.base_flags.get("spell").copied().unwrap_or(false);
                            let is_attack = s.base_flags.get("attack").copied().unwrap_or(false);
                            if is_spell && !state.show_spells {
                                return false;
                            }
                            if is_attack && !state.show_attacks {
                                return false;
                            }
                            if !is_spell && !is_attack {
                                return false;
                            }
                            if q.is_empty() {
                                return true;
                            }
                            s.name.to_lowercase().contains(&q)
                        })
                        .collect();
                    skills.sort_by(|a, b| a.1.name.cmp(&b.1.name));
                    for (id, s) in skills {
                        let selected = main_skill
                            .as_ref()
                            .map(|m| m.skill_id == id)
                            .unwrap_or(false);
                        let label = format!("{}", s.name);
                        if ui.selectable_label(selected, label).clicked() {
                            *main_skill = Some(MainSkill::new(id));
                            changed = true;
                        }
                    }
                });
        });

        ui.separator();

        // Skill detail / level / quality
        ui.vertical(|ui| {
            if let Some(main) = main_skill.as_mut() {
                if let Some(skill) = registry.get(&main.skill_id) {
                    ui.heading(&skill.name);
                    ui.label(format!("ID: {}", main.skill_id));
                    let flags: Vec<&str> = skill
                        .base_flags
                        .iter()
                        .filter(|(_, on)| **on)
                        .map(|(k, _)| k.as_str())
                        .collect();
                    ui.label(format!("Flags: {}", flags.join(", ")));
                    ui.label(format!("Cast time: {:.2}s", skill.cast_time));
                    ui.label(format!("Levels available: {}", skill.levels.len()));
                    ui.add_space(6.0);
                    let max_level = skill.levels.len().max(1).min(40) as u32;
                    let prev_level = main.level;
                    ui.add(
                        egui::Slider::new(&mut main.level, 1..=max_level)
                            .text("Gem level"),
                    );
                    if main.level != prev_level {
                        changed = true;
                    }
                    let prev_q = main.quality;
                    ui.add(egui::Slider::new(&mut main.quality, 0..=23).text("Quality %"));
                    if main.quality != prev_q {
                        changed = true;
                    }
                    ui.add_space(6.0);
                    if !skill.description.is_empty() {
                        ui.label(egui::RichText::new(&skill.description).italics());
                    }
                    // Tag chips
                    if !skill.skill_types.is_empty() {
                        let tag_count = skill.skill_types.iter().filter(|(_, on)| **on).count();
                        ui.label(format!("Skill types: {tag_count}"));
                    }
                    if !skill.constant_stats.is_empty() {
                        ui.add_space(4.0);
                        ui.collapsing("Constant stats", |ui| {
                            for s in &skill.constant_stats {
                                ui.monospace(s.to_string());
                            }
                        });
                    }
                    if !skill.quality_stats.is_empty() {
                        ui.collapsing("Quality stats (per +1% Q)", |ui| {
                            for s in &skill.quality_stats {
                                ui.monospace(s.to_string());
                            }
                        });
                    }
                    if !skill.stats.is_empty() {
                        ui.collapsing("Per-level stat ids", |ui| {
                            for s in &skill.stats {
                                ui.monospace(s);
                            }
                        });
                    }
                } else {
                    ui.colored_label(
                        egui::Color32::LIGHT_RED,
                        format!("Skill `{}` not found in registry", main.skill_id),
                    );
                    if ui.button("Clear selection").clicked() {
                        *main_skill = None;
                        changed = true;
                    }
                }
            } else {
                ui.label("Pick a skill on the left.");
            }
        });
    });

    changed
}
