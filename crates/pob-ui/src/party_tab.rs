//! Party tab — group-play teammates whose auras / curses / banners
//! project onto the player. Each member carries a free-form list of
//! mod lines; the engine parses them through `mod_parser` at compute
//! time and tags them with `Party:<member_name>` as the source.
//!
//! This MVP doesn't yet import a teammate's PoB code and extract their
//! actual auras — the user pastes the relevant mod lines directly.
//! Future work: link to a `pob_engine::import_pob_code` call that
//! pulls auras out of a teammate's tree + skill bar automatically.

use eframe::egui;
use pob_data::{bases::ItemBaseSet, PassiveTree};
use pob_engine::{
    character::{ExtractedAura, PartyMember},
    Character, SkillRegistry,
};

#[derive(Debug, Clone, Default)]
pub struct PartyTabState {
    /// Index of the member currently shown in the right-pane editor.
    /// `None` means "no member selected".
    pub selected: Option<usize>,
    /// Buffer for the "add new member" name input.
    pub new_name: String,
    /// Buffer for the per-member "paste teammate PoB code" textarea.
    /// Indexed by member index — keeps the user's draft visible while
    /// they work in other tabs without polluting `Character`.
    pub import_buffers: Vec<String>,
    /// Status line from the most recent import attempt (success or
    /// parse failure). Cleared when the user types into the buffer.
    pub import_status: Vec<Option<String>>,
}

/// Returns true if any member field changed (so the caller can recompute).
///
/// `tree` and `bases` are threaded through for issue #97 slice 3:
/// the auto-extraction path runs `compute_full` on the teammate's
/// imported character to derive their effective AuraEffect % at
/// extract time, so the projected mods reflect Generosity Support
/// / item AuraEffect bonuses without the user having to dial them
/// in by hand. They're optional in spirit (the parent app always
/// has them today, but threading them as required keeps the
/// signature honest).
pub fn ui(
    ui: &mut egui::Ui,
    state: &mut PartyTabState,
    character: &mut Character,
    registry: &SkillRegistry,
    tree: &PassiveTree,
    bases: Option<&ItemBaseSet>,
) -> bool {
    let mut changed = false;
    // Keep the per-member buffer arrays sized to the member list. Adds /
    // removes mutate `party_members` first; we resize lazily here so
    // both buffers and statuses index parallel to the member vec.
    state
        .import_buffers
        .resize(character.party_members.len(), String::new());
    state
        .import_status
        .resize(character.party_members.len(), None);

    ui.horizontal(|ui| {
        ui.heading("Party");
        ui.separator();
        ui.add(
            egui::TextEdit::singleline(&mut state.new_name)
                .desired_width(160.0)
                .hint_text("Teammate name…"),
        );
        let add_enabled = !state.new_name.trim().is_empty();
        if ui
            .add_enabled(add_enabled, egui::Button::new("Add member"))
            .clicked()
        {
            character.party_members.push(PartyMember {
                name: state.new_name.trim().to_owned(),
                mod_lines: String::new(),
                extracted_auras: Vec::new(),
                enabled: true,
            });
            state.import_buffers.push(String::new());
            state.import_status.push(None);
            state.selected = Some(character.party_members.len() - 1);
            state.new_name.clear();
            changed = true;
        }
    });
    ui.separator();

    if character.party_members.is_empty() {
        ui.label(
            "No party members yet. Add one above and paste the mod lines they project (auras, \
             banners, curses, etc.) into the right pane.\n\
             \n\
             Example: a teammate running Hatred at 50% effect would have a line like\n\
             `+50% to Cold Damage` in their mod box; the player's Calcs tab then\n\
             shows the buff sourced from `Party:<their name>`.",
        );
        return changed;
    }

    ui.horizontal(|ui| {
        // Left pane: member list with enable toggle and select / delete.
        ui.vertical(|ui| {
            ui.set_min_width(200.0);
            // Snapshot so we don't borrow during mutation inside the loop.
            let entries: Vec<(usize, String, bool)> = character
                .party_members
                .iter()
                .enumerate()
                .map(|(i, m)| (i, m.name.clone(), m.enabled))
                .collect();
            let mut delete_idx: Option<usize> = None;
            let mut toggle_idx: Option<usize> = None;
            for (idx, name, enabled) in entries {
                ui.horizontal(|ui| {
                    let label = if enabled {
                        name.clone()
                    } else {
                        format!("{name} (off)")
                    };
                    let selected = state.selected == Some(idx);
                    if ui.selectable_label(selected, label).clicked() {
                        state.selected = Some(idx);
                    }
                    if ui
                        .small_button(if enabled { "✓" } else { "—" })
                        .on_hover_text("Toggle this member's contribution")
                        .clicked()
                    {
                        toggle_idx = Some(idx);
                    }
                    if ui
                        .small_button("✕")
                        .on_hover_text("Remove member")
                        .clicked()
                    {
                        delete_idx = Some(idx);
                    }
                });
            }
            if let Some(idx) = toggle_idx {
                if let Some(member) = character.party_members.get_mut(idx) {
                    member.enabled = !member.enabled;
                    changed = true;
                }
            }
            if let Some(idx) = delete_idx {
                character.party_members.remove(idx);
                if idx < state.import_buffers.len() {
                    state.import_buffers.remove(idx);
                }
                if idx < state.import_status.len() {
                    state.import_status.remove(idx);
                }
                if state.selected == Some(idx) {
                    state.selected = None;
                } else if let Some(sel) = state.selected {
                    if sel > idx {
                        state.selected = Some(sel - 1);
                    }
                }
                changed = true;
            }
        });

        ui.separator();

        // Right pane: edit selected member's mod lines.
        ui.vertical(|ui| {
            let Some(idx) = state.selected else {
                ui.weak("Select a member on the left to edit their mods.");
                return;
            };
            let total_members = character.party_members.len();
            let Some(member) = character.party_members.get_mut(idx) else {
                state.selected = None;
                return;
            };
            ui.horizontal(|ui| {
                ui.label("Member:");
                let resp =
                    ui.add(egui::TextEdit::singleline(&mut member.name).desired_width(220.0));
                if resp.changed() {
                    changed = true;
                }
                ui.weak(format!("({}/{})", idx + 1, total_members));
            });
            ui.checkbox(&mut member.enabled, "Active in calc")
                .on_hover_text("Untick to exclude this member from the next compute pass");

            // Issue #97: extracted auras / curses / banners list.
            // Each entry can be toggled or removed inline; the
            // engine's `apply_party_extracted_auras` consumes this
            // list at compute time.
            if !member.extracted_auras.is_empty() {
                ui.add_space(6.0);
                ui.label(egui::RichText::new("Auto-extracted gems").strong());
                let mut to_remove: Option<usize> = None;
                for (i, aura) in member.extracted_auras.iter_mut().enumerate() {
                    ui.horizontal(|ui| {
                        if ui.checkbox(&mut aura.enabled, "").changed() {
                            changed = true;
                        }
                        let display = registry
                            .get(&aura.skill_id)
                            .map(|s| s.name.as_str())
                            .unwrap_or(&aura.skill_id);
                        ui.label(format!(
                            "{display}  L{level} Q{quality}%",
                            level = aura.level,
                            quality = aura.quality
                        ));
                        // Issue #97 (slice 2): manual aura-effect %
                        // override. Lets the user dial in the effect
                        // boost from the teammate's Generosity /
                        // Empower / aura-effect items without
                        // re-running their full calc here. Negative
                        // is allowed but clamped at -100% downstream.
                        if ui
                            .add(
                                egui::DragValue::new(&mut aura.effect_pct)
                                    .speed(1.0)
                                    .range(-100..=300)
                                    .suffix("% effect"),
                            )
                            .on_hover_text(
                                "Aura-effect % the teammate has on this aura \
                                 (Generosity, +X% Aura Effect mods, etc.). \
                                 0% = use the gem's raw L/Q values.",
                            )
                            .changed()
                        {
                            changed = true;
                        }
                        if ui.small_button("✕").clicked() {
                            to_remove = Some(i);
                        }
                    });
                }
                if let Some(rm) = to_remove {
                    member.extracted_auras.remove(rm);
                    changed = true;
                }
            }

            ui.label("Mod lines (one per line, same syntax as Custom Modifiers / item mods):");
            let resp = ui.add(
                egui::TextEdit::multiline(&mut member.mod_lines)
                    .desired_width(f32::INFINITY)
                    .desired_rows(10)
                    .hint_text("e.g. +30% to Cold Damage\n+15% increased Attack Speed")
                    .font(egui::TextStyle::Monospace),
            );
            if resp.changed() {
                changed = true;
            }
            // Live parse-status summary so users can spot bad lines at
            // a glance — same UX as Custom Modifiers.
            let total = member
                .mod_lines
                .lines()
                .filter(|l| !l.trim().is_empty())
                .count();
            if total > 0 {
                let parsed = member
                    .mod_lines
                    .lines()
                    .filter(|l| !l.trim().is_empty())
                    .filter(|l| pob_engine::mod_parser::parse_mod_line(l.trim()).is_some())
                    .count();
                let color = if parsed == total {
                    egui::Color32::from_rgb(0x33, 0xFF, 0x77)
                } else {
                    egui::Color32::from_rgb(0xFF, 0x99, 0x22)
                };
                ui.colored_label(color, format!("{parsed} / {total} lines parse"));
            }

            // Issue #97: paste-and-extract panel. Drop a teammate's
            // PoB code (or `<PathOfBuilding>` XML) into the box and
            // press "Extract auras" — the engine pulls every aura /
            // curse / banner gem from their skill bar and prepopulates
            // the `extracted_auras` list above.
            ui.add_space(8.0);
            ui.separator();
            ui.label(egui::RichText::new("Auto-extract from teammate PoB code").strong());
            let buf = state
                .import_buffers
                .get_mut(idx)
                .expect("buffer slot resized at top of ui()");
            let resp = ui.add(
                egui::TextEdit::multiline(buf)
                    .desired_width(f32::INFINITY)
                    .desired_rows(4)
                    .hint_text("Paste pobb.in / pob.cool code or PoB XML here")
                    .font(egui::TextStyle::Monospace),
            );
            if resp.changed() {
                if let Some(slot) = state.import_status.get_mut(idx) {
                    *slot = None;
                }
            }
            ui.horizontal(|ui| {
                if ui
                    .add_enabled(!buf.trim().is_empty(), egui::Button::new("Extract auras"))
                    .clicked()
                {
                    let import = run_import(&buf.trim().to_owned());
                    match import {
                        Ok(extracted) => {
                            let count = extract_into(member, &extracted, registry, tree, bases);
                            if let Some(slot) = state.import_status.get_mut(idx) {
                                *slot = Some(format!("Extracted {count} aura/curse/banner gem(s)"));
                            }
                            // Clear the buffer once the extraction
                            // succeeds so users don't double-import.
                            buf.clear();
                            changed = true;
                        }
                        Err(e) => {
                            if let Some(slot) = state.import_status.get_mut(idx) {
                                *slot = Some(format!("Import failed: {e}"));
                            }
                        }
                    }
                }
                if ui.small_button("Clear buffer").clicked() {
                    buf.clear();
                    if let Some(slot) = state.import_status.get_mut(idx) {
                        *slot = None;
                    }
                }
            });
            if let Some(Some(msg)) = state.import_status.get(idx) {
                let is_err = msg.starts_with("Import failed");
                let color = if is_err {
                    egui::Color32::from_rgb(0xFF, 0x99, 0x22)
                } else {
                    egui::Color32::from_rgb(0x33, 0xFF, 0x77)
                };
                ui.colored_label(color, msg);
            }
        });
    });

    changed
}

/// Try every PoB import path the user might paste: a PoB share code
/// (zlib + base64), a `<PathOfBuilding>` XML document, or — as a
/// convenience — an MK2 share code. Returns the parsed teammate
/// `Character` on success.
fn run_import(input: &str) -> Result<pob_engine::Character, String> {
    let trimmed = input.trim();
    if trimmed.starts_with("MK2|") {
        return pob_engine::import_code(trimmed).map_err(|e| e.to_string());
    }
    if trimmed.starts_with('<') {
        return pob_engine::import_pob_xml(trimmed).map_err(|e| e.to_string());
    }
    pob_engine::import_pob_code(trimmed).map_err(|e| e.to_string())
}

/// Walk the teammate `Character`'s socket groups, find every enabled
/// gem whose `baseFlags` include `aura` / `curse` / `banner`, and
/// append them to `member.extracted_auras`. Returns the number of
/// gems added. Existing entries with the same `skill_id` are
/// replaced so re-importing a refreshed teammate code updates levels
/// instead of duplicating gems.
fn extract_into(
    member: &mut PartyMember,
    teammate: &pob_engine::Character,
    registry: &SkillRegistry,
    tree: &PassiveTree,
    bases: Option<&ItemBaseSet>,
) -> usize {
    // Issue #97 (slice 3): derive the teammate's effective
    // AuraEffect % by running the full calc pipeline on their
    // imported character. The mod_db at the end of compute_full
    // carries every aura-effect contribution from items, supports
    // (Generosity), and tree nodes — summed as INC + MORE here
    // and converted to a single `effect_pct` scalar PoB-style:
    //   scalar = (1 + AuraEffect_inc/100) × AuraEffect_more
    // We round to the nearest integer percent for storage in the
    // ExtractedAura slot (slice 2's user-overridable knob). The
    // user can still hand-edit per aura after extraction.
    let auto_effect_pct = teammate_aura_effect_pct(teammate, tree, registry, bases);

    let mut added = 0;
    for group in &teammate.skill_groups {
        if !group.enabled {
            continue;
        }
        for gem in &group.gems {
            if !gem.enabled {
                continue;
            }
            let Some(skill) = registry.get(&gem.skill_id) else {
                continue;
            };
            let is_projection = skill.base_flags.get("aura").copied().unwrap_or(false)
                || skill.base_flags.get("curse").copied().unwrap_or(false)
                || skill.base_flags.get("banner").copied().unwrap_or(false);
            if !is_projection {
                continue;
            }
            // Issue #97 (slice 4): preserve user edits on re-import.
            // The teammate-level data (skill_id, level, quality)
            // gets refreshed from the new paste, but the user's
            // manually-flipped `enabled` toggle and any
            // hand-overridden `effect_pct` survive across
            // re-extractions. Only auras the user hadn't seen
            // before pick up the auto-detected effect_pct from
            // the slice-3 path.
            if let Some(existing) = member
                .extracted_auras
                .iter_mut()
                .find(|a| a.skill_id == gem.skill_id)
            {
                existing.level = gem.level.max(1);
                existing.quality = gem.quality;
                // `enabled` and `effect_pct` deliberately not
                // touched — those carry the user's intent.
            } else {
                member.extracted_auras.push(ExtractedAura {
                    skill_id: gem.skill_id.clone(),
                    level: gem.level.max(1),
                    quality: gem.quality,
                    enabled: true,
                    effect_pct: auto_effect_pct,
                });
                added += 1;
            }
        }
    }
    added
}

/// Issue #97 (slice 3): compute the teammate's effective
/// AuraEffect % at the moment of extraction. Runs `compute_full`
/// on the imported character and reads the AuraEffect INC + MORE
/// totals from the resulting modDB. Returns a rounded percent
/// suitable for `ExtractedAura.effect_pct` (`+30` = 1.30× scalar
/// applied to projected mod values). Any compute path that fails
/// (no tree, no skills loaded) falls back to 0.
fn teammate_aura_effect_pct(
    teammate: &pob_engine::Character,
    tree: &PassiveTree,
    registry: &SkillRegistry,
    bases: Option<&ItemBaseSet>,
) -> i32 {
    use pob_engine::{ModStore as _, ModType};
    let (_out, env) = pob_engine::compute_full_with_env(teammate, tree, Some(registry), bases);
    let cfg = pob_engine::mod_db::QueryCfg::default();
    let inc = env.mod_db.sum(ModType::Inc, &cfg, &env.state, "AuraEffect");
    let more = env.mod_db.more(&cfg, &env.state, "AuraEffect");
    let scalar = (1.0 + inc / 100.0) * more;
    // Convert to a percent delta from 1.0 and round.
    ((scalar - 1.0) * 100.0).round() as i32
}
