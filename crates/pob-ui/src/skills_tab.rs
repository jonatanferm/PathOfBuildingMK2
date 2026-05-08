//! Skills tab — manage skill gem groups and pick the main active skill.
//!
//! Layout:
//! - Left: socket-group list with add/remove buttons
//! - Middle: gems within the selected group
//! - Right: skill catalog (filterable) + level/quality sliders for the
//!   currently-selected gem

use eframe::egui;
use pob_engine::character::SocketGroup;
use pob_engine::{Character, MainSkill, SkillRegistry};

use crate::color_codes;

pub struct SkillsTabState {
    pub filter: String,
    pub show_spells: bool,
    pub show_attacks: bool,
    pub show_supports: bool,
    pub selected_group: usize,
    pub selected_gem: usize,
    pub catalog_open: bool,
}

impl Default for SkillsTabState {
    fn default() -> Self {
        Self {
            filter: String::new(),
            show_spells: true,
            show_attacks: true,
            show_supports: true,
            selected_group: 0,
            selected_gem: 0,
            catalog_open: false,
        }
    }
}

pub fn ui(
    ui: &mut egui::Ui,
    state: &mut SkillsTabState,
    character: &mut Character,
    registry: &SkillRegistry,
) -> bool {
    let mut changed = false;

    // Ensure there's always at least one group so the user can socket gems.
    if character.skill_groups.is_empty() {
        character.skill_groups.push(SocketGroup {
            label: "Group 1".into(),
            gems: Vec::new(),
            main_active_skill_index: 1,
            enabled: true,
        });
        character.main_socket_group = 1;
    }
    state.selected_group = state
        .selected_group
        .min(character.skill_groups.len().saturating_sub(1));

    ui.horizontal(|ui| {
        // ── Group list ──────────────────────────────────────────────────────
        ui.vertical(|ui| {
            ui.set_min_width(160.0);
            ui.heading("Groups");
            ui.separator();
            let mut to_remove: Option<usize> = None;
            for (idx, group) in character.skill_groups.iter_mut().enumerate() {
                ui.horizontal(|ui| {
                    let main_marker = if (idx as u32 + 1) == character.main_socket_group {
                        "★"
                    } else {
                        " "
                    };
                    let label = if group.label.is_empty() {
                        format!("{} Group {}", main_marker, idx + 1)
                    } else {
                        format!("{} {}", main_marker, group.label)
                    };
                    let display = format!(
                        "{label} ({} gem{})",
                        group.gems.len(),
                        if group.gems.len() == 1 { "" } else { "s" }
                    );
                    if ui
                        .selectable_label(state.selected_group == idx, display)
                        .clicked()
                    {
                        state.selected_group = idx;
                        state.selected_gem = 0;
                    }
                    if ui.small_button("✕").on_hover_text("Remove group").clicked() {
                        to_remove = Some(idx);
                    }
                });
            }
            if let Some(rm) = to_remove {
                character.skill_groups.remove(rm);
                if state.selected_group >= character.skill_groups.len() {
                    state.selected_group = character.skill_groups.len().saturating_sub(1);
                }
                changed = true;
            }
            ui.separator();
            if ui.button("➕ New group").clicked() {
                character.skill_groups.push(SocketGroup {
                    label: format!("Group {}", character.skill_groups.len() + 1),
                    gems: Vec::new(),
                    main_active_skill_index: 1,
                    enabled: true,
                });
                state.selected_group = character.skill_groups.len() - 1;
                changed = true;
            }
            ui.add_space(8.0);
            ui.label("Main socket group:");
            let mut current_main = character.main_socket_group;
            let label = if let Some(g) = character
                .skill_groups
                .get((current_main as usize).saturating_sub(1))
            {
                if g.label.is_empty() {
                    format!("Group {current_main}")
                } else {
                    g.label.clone()
                }
            } else {
                "(none)".into()
            };
            egui::ComboBox::from_id_salt("main_group_combo")
                .selected_text(label)
                .show_ui(ui, |ui| {
                    for (idx, g) in character.skill_groups.iter().enumerate() {
                        let one_based = (idx + 1) as u32;
                        let txt = if g.label.is_empty() {
                            format!("Group {one_based}")
                        } else {
                            g.label.clone()
                        };
                        if ui
                            .selectable_label(current_main == one_based, txt)
                            .clicked()
                        {
                            current_main = one_based;
                        }
                    }
                });
            if current_main != character.main_socket_group {
                character.main_socket_group = current_main;
                changed = true;
            }
        });

        ui.separator();

        // ── Selected group editor ───────────────────────────────────────────
        ui.vertical(|ui| {
            ui.set_min_width(280.0);
            let Some(group) = character.skill_groups.get_mut(state.selected_group) else {
                ui.label("Pick a group on the left.");
                return;
            };
            ui.horizontal(|ui| {
                ui.heading("Gems");
                ui.separator();
                if ui.checkbox(&mut group.enabled, "Enabled").changed() {
                    changed = true;
                }
            });
            ui.label("Group label:");
            if ui
                .add(
                    egui::TextEdit::singleline(&mut group.label)
                        .desired_width(220.0)
                        .hint_text("Group 1"),
                )
                .changed()
            {
                changed = true;
            }
            ui.separator();
            // Gem list
            let mut to_remove_gem: Option<usize> = None;
            for (idx, gem) in group.gems.iter_mut().enumerate() {
                let one_based = (idx as u32) + 1;
                let main_marker = if one_based == group.main_active_skill_index {
                    "★"
                } else {
                    " "
                };
                let skill_meta = registry.get(&gem.skill_id);
                let is_support = skill_meta.map(|s| s.support).unwrap_or(false);
                let kind_marker = if is_support { "⚙" } else { " " };
                let display_name = skill_meta.map(|s| s.name.as_str()).unwrap_or(&gem.skill_id);
                let label = format!(
                    "{} {} {} (L{} Q{}%)",
                    main_marker, kind_marker, display_name, gem.level, gem.quality
                );
                ui.horizontal(|ui| {
                    if ui
                        .checkbox(&mut gem.enabled, "")
                        .on_hover_text("Enable / disable this gem")
                        .changed()
                    {
                        changed = true;
                    }
                    let label_text = if gem.enabled {
                        egui::RichText::new(&label)
                    } else {
                        egui::RichText::new(&label).weak().strikethrough()
                    };
                    if ui
                        .selectable_label(state.selected_gem == idx, label_text)
                        .clicked()
                    {
                        state.selected_gem = idx;
                    }
                    if ui
                        .small_button("★")
                        .on_hover_text("Set as main skill")
                        .clicked()
                    {
                        group.main_active_skill_index = one_based;
                        changed = true;
                    }
                    if ui.small_button("✕").on_hover_text("Remove gem").clicked() {
                        to_remove_gem = Some(idx);
                    }
                });
            }
            if let Some(rm) = to_remove_gem {
                group.gems.remove(rm);
                if state.selected_gem >= group.gems.len() {
                    state.selected_gem = group.gems.len().saturating_sub(1);
                }
                if group.main_active_skill_index > group.gems.len() as u32 {
                    group.main_active_skill_index = 1;
                }
                changed = true;
            }
            ui.separator();
            if ui.button("➕ Socket gem from catalog").clicked() {
                state.catalog_open = true;
            }

            // Selected-gem details
            if let Some(gem) = group.gems.get_mut(state.selected_gem) {
                ui.add_space(6.0);
                ui.label(egui::RichText::new(&gem.skill_id).strong());

                // Issue #36 (slice 2): variant picker. `SkillRegistry::variants_of`
                // surfaces every gem id sharing the same base — Vaal counterparts
                // and `AltX/Y/A/B/C` siblings. We only render the dropdown if
                // there's actually more than one variant (i.e. the gem has a
                // Vaal/alt-quality option). Picking a new variant rewrites
                // `gem.skill_id`; the level/quality sliders below pick up the
                // new entry's `levels` table next frame.
                let variants: Vec<String> = registry
                    .variants_of(&gem.skill_id)
                    .into_iter()
                    .map(str::to_owned)
                    .collect();
                if variants.len() > 1 {
                    let mut chosen = gem.skill_id.clone();
                    egui::ComboBox::from_label("Variant")
                        .selected_text(&chosen)
                        .show_ui(ui, |ui| {
                            for v in &variants {
                                ui.selectable_value(&mut chosen, v.clone(), v);
                            }
                        });
                    if chosen != gem.skill_id {
                        gem.skill_id = chosen;
                        // Variants share the same level table shape (PoB tables
                        // mirror length between primary/secondary), but defensively
                        // clamp to whatever the new entry advertises.
                        if let Some(s) = registry.get(&gem.skill_id) {
                            let max_level = s.levels.len().max(1).min(40) as u32;
                            gem.level = gem.level.clamp(1, max_level);
                        }
                        changed = true;
                    }
                }

                if let Some(skill) = registry.get(&gem.skill_id) {
                    let max_level = skill.levels.len().max(1).min(40) as u32;
                    let prev_level = gem.level;
                    if ui
                        .add(egui::Slider::new(&mut gem.level, 1..=max_level).text("Gem level"))
                        .changed()
                        || gem.level != prev_level
                    {
                        if gem.level != prev_level {
                            changed = true;
                        }
                    }
                    let prev_q = gem.quality;
                    if ui
                        .add(egui::Slider::new(&mut gem.quality, 0..=23).text("Quality %"))
                        .changed()
                        || gem.quality != prev_q
                    {
                        if gem.quality != prev_q {
                            changed = true;
                        }
                    }
                    if !skill.description.is_empty() {
                        ui.add_space(4.0);
                        // Gem descriptions in upstream PoB carry inline
                        // `^N` / `^xRRGGBB` color escapes (e.g. damage
                        // numbers in yellow). Render them faithfully;
                        // fall back to a muted weak text colour when
                        // there are no escapes so the description still
                        // visually separates from the gem name above.
                        let default = ui.style().visuals.weak_text_color();
                        let font = egui::TextStyle::Body.resolve(ui.style());
                        let job = color_codes::to_layout_job(&skill.description, default, font);
                        ui.label(job);
                    }
                }
            }
        });

        // ── Catalog / picker ────────────────────────────────────────────────
        if state.catalog_open {
            ui.separator();
            ui.vertical(|ui| {
                ui.set_min_width(320.0);
                ui.horizontal(|ui| {
                    ui.heading("Catalog");
                    if ui.button("Close").clicked() {
                        state.catalog_open = false;
                    }
                });
                ui.horizontal(|ui| {
                    ui.label("Filter:");
                    ui.add(
                        egui::TextEdit::singleline(&mut state.filter)
                            .desired_width(200.0)
                            .hint_text("Arc, Fireball, …"),
                    );
                });
                ui.horizontal(|ui| {
                    ui.checkbox(&mut state.show_spells, "Spells");
                    ui.checkbox(&mut state.show_attacks, "Attacks");
                    ui.checkbox(&mut state.show_supports, "Supports");
                });
                ui.label(format!("{} skills loaded", registry.len()));
                ui.separator();
                let q = state.filter.trim().to_lowercase();
                let mut skills: Vec<(&str, &pob_data::Skill)> = registry
                    .iter_active()
                    .filter(|(_, s)| {
                        let is_spell = s.base_flags.get("spell").copied().unwrap_or(false);
                        let is_attack = s.base_flags.get("attack").copied().unwrap_or(false);
                        let is_support = s.support;
                        if is_support && !state.show_supports {
                            return false;
                        }
                        if !is_support {
                            if is_spell && !state.show_spells {
                                return false;
                            }
                            if is_attack && !state.show_attacks {
                                return false;
                            }
                            if !is_spell && !is_attack {
                                return false;
                            }
                        }
                        if q.is_empty() {
                            return true;
                        }
                        s.name.to_lowercase().contains(&q)
                    })
                    .collect();
                skills.sort_by(|a, b| a.1.name.cmp(&b.1.name));
                egui::ScrollArea::vertical()
                    .id_salt("skill_catalog")
                    .auto_shrink([false, false])
                    .max_height(420.0)
                    .show(ui, |ui| {
                        for (id, s) in skills {
                            let label = if s.support {
                                format!("⚙ {}", s.name)
                            } else {
                                s.name.clone()
                            };
                            if ui.selectable_label(false, label).clicked() {
                                if let Some(group) =
                                    character.skill_groups.get_mut(state.selected_group)
                                {
                                    group.gems.push(MainSkill::new(id));
                                    state.selected_gem = group.gems.len() - 1;
                                    if group.main_active_skill_index == 0 {
                                        group.main_active_skill_index = 1;
                                    }
                                    changed = true;
                                    state.catalog_open = false;
                                }
                            }
                        }
                    });
            });
        }
    });

    if changed {
        // Re-derive `main_skill` from the active group/gem so the calc layer
        // sees the user's current selection without us threading two paths.
        character.sync_main_skill();
    }
    changed
}
