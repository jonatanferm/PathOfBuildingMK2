//! Items tab — slot grid + paste-to-equip.

use eframe::egui;
use pob_data::{Item, ItemSet, Rarity, Slot};
use pob_engine::{parse_item, Character};

use crate::color_codes;

pub struct ItemsTabState {
    /// Slot the user is currently editing (paste / clear / view).
    pub selected_slot: Option<Slot>,
    /// Buffer for the textarea input.
    pub paste_buffer: String,
    /// Last parse error, if any, shown next to the textarea.
    pub last_error: Option<String>,
    /// Buffer for the "save current as new set" name input.
    pub new_set_name: String,
}

impl Default for ItemsTabState {
    fn default() -> Self {
        Self {
            selected_slot: Some(Slot::Amulet),
            paste_buffer: String::new(),
            last_error: None,
            new_set_name: String::new(),
        }
    }
}

/// Returns true if the equipped items changed (so the caller can recompute).
pub fn ui(ui: &mut egui::Ui, state: &mut ItemsTabState, character: &mut Character) -> bool {
    let mut changed = false;
    // Issue #27: item-set saves. Top row lets the user save the current
    // loadout as a named set, swap a saved set in, or delete one.
    ui.horizontal_wrapped(|ui| {
        ui.label("Item sets:");
        let total = character.item_sets.len();
        if total == 0 {
            ui.weak("(none saved)");
        } else {
            // Snapshot names so we don't borrow `character.item_sets`
            // while mutating it inside the loop.
            let entries: Vec<(usize, String)> = character
                .item_sets
                .iter()
                .enumerate()
                .map(|(i, s)| (i, s.name.clone()))
                .collect();
            for (idx, name) in entries {
                if ui.button(format!("Load {name}")).clicked() {
                    if character.activate_item_set(idx) {
                        changed = true;
                    }
                }
                if ui
                    .small_button("✕")
                    .on_hover_text(format!("Delete {name}"))
                    .clicked()
                {
                    if character.delete_item_set(idx) {
                        // No recompute — deleting a saved (inactive)
                        // set doesn't change `character.items`.
                    }
                }
            }
        }
        ui.separator();
        ui.add(
            egui::TextEdit::singleline(&mut state.new_set_name)
                .desired_width(120.0)
                .hint_text("New set name…"),
        );
        let save_enabled = !state.new_set_name.trim().is_empty();
        if ui
            .add_enabled(save_enabled, egui::Button::new("Save current as set"))
            .clicked()
        {
            character.save_item_set(state.new_set_name.trim().to_owned());
            state.new_set_name.clear();
        }
    });
    ui.separator();

    // Issue #109 (slice 4): Weapon Set I / II toggle buttons.
    // Mirrors PoB's `Classes/ItemsTab.lua:208-247` weaponSwap1 /
    // weaponSwap2 buttons that flip `useSecondWeaponSet` on the
    // active item set. We expose them on the items tab itself so
    // users don't have to dive into the Config tab to swap. The
    // active button is rendered as "selected" so it's obvious which
    // pair is currently driving the calc engine.
    ui.horizontal(|ui| {
        ui.label("Weapon Set:");
        let active_one = !character.config.use_second_weapon_set;
        if ui
            .selectable_label(active_one, "I (Primary)")
            .on_hover_text("Use the primary weapon pair (Weapon 1 / Weapon 2) as the live pair.")
            .clicked()
            && !active_one
        {
            character.config.use_second_weapon_set = false;
            changed = true;
        }
        if ui
            .selectable_label(!active_one, "II (Swap)")
            .on_hover_text(
                "Use the swap weapon pair (Weapon 1 Swap / Weapon 2 Swap) as the live \
                 pair. Mirrors PoB's X-key weapon swap. Useful for caster off-hand-buff \
                 stacking and Storm Brand swap-trap setups.",
            )
            .clicked()
            && active_one
        {
            character.config.use_second_weapon_set = true;
            changed = true;
        }
        if !active_one {
            ui.weak("(swap pair is live)");
        }
    });
    ui.separator();

    let use_swap = character.config.use_second_weapon_set;
    let items: &mut ItemSet = &mut character.items;
    ui.horizontal(|ui| {
        // Left: slot grid
        ui.vertical(|ui| {
            ui.set_min_width(180.0);
            ui.heading("Slots");
            ui.separator();
            // Issue #109 (slice 3): visually separate the swap pair
            // from the primary pair so it's clear which entries the
            // calc engine reads when `use_second_weapon_set` is on.
            // We render the slot list in three groups: primary
            // equipment, the swap pair, and flasks. Slice 4 marks
            // the active pair with a small marker so the user
            // doesn't have to mentally cross-reference the toggle.
            let is_primary_weapon = |s: &Slot| matches!(s, Slot::Weapon1 | Slot::Weapon2);
            let is_swap = |s: &Slot| matches!(s, Slot::Weapon1Swap | Slot::Weapon2Swap);
            let is_flask = |s: &Slot| {
                matches!(
                    s,
                    Slot::Flask1 | Slot::Flask2 | Slot::Flask3 | Slot::Flask4 | Slot::Flask5
                )
            };
            let render_slot =
                |ui: &mut egui::Ui, slot: &Slot, state: &mut ItemsTabState, inactive: bool| {
                    let equipped = items.get(*slot);
                    let label = if let Some(item) = equipped {
                        let rarity_glyph = rarity_glyph(item.rarity);
                        if item.name.is_empty() {
                            format!("{rarity_glyph} {} — {}", slot.label(), item.base_name)
                        } else {
                            format!("{rarity_glyph} {} — {}", slot.label(), item.name)
                        }
                    } else {
                        format!("· {} — (empty)", slot.label())
                    };
                    let selected = state.selected_slot == Some(*slot);
                    let response = if inactive {
                        // Dim weapons that are *not* the current live pair so it's
                        // clear at a glance which weapons drive the calc engine.
                        let dim = egui::RichText::new(label).weak();
                        ui.selectable_label(selected, dim)
                    } else {
                        ui.selectable_label(selected, label)
                    };
                    if response.clicked() {
                        state.selected_slot = Some(*slot);
                        state.paste_buffer.clear();
                        state.last_error = None;
                    }
                };
            for slot in Slot::all() {
                if is_swap(slot) || is_flask(slot) {
                    continue;
                }
                let inactive = is_primary_weapon(slot) && use_swap;
                render_slot(ui, slot, state, inactive);
            }
            ui.add_space(4.0);
            if use_swap {
                ui.label(egui::RichText::new("Swap weapon set (live)").strong());
            } else {
                ui.weak("Swap weapon set");
            }
            for slot in Slot::all() {
                if !is_swap(slot) {
                    continue;
                }
                let inactive = !use_swap;
                render_slot(ui, slot, state, inactive);
            }
            ui.add_space(4.0);
            ui.weak("Flasks");
            for slot in Slot::all() {
                if !is_flask(slot) {
                    continue;
                }
                render_slot(ui, slot, state, false);
            }
        });

        ui.separator();

        // Right: editor
        ui.vertical(|ui| {
            ui.heading(
                state
                    .selected_slot
                    .map(|s| s.label().to_string())
                    .unwrap_or_else(|| "(no slot selected)".to_owned()),
            );
            ui.separator();

            if let Some(slot) = state.selected_slot {
                if let Some(item) = items.get(slot) {
                    egui::ScrollArea::vertical()
                        .max_height(220.0)
                        .show(ui, |ui| {
                            render_item_summary(ui, item);
                        });
                    ui.add_space(4.0);
                    if ui.button("Unequip").clicked() {
                        items.unequip(slot);
                        changed = true;
                    }
                    ui.separator();
                }
                ui.label("Paste an item from PoE / PoB:");
                egui::ScrollArea::vertical()
                    .max_height(220.0)
                    .show(ui, |ui| {
                        ui.add(
                            egui::TextEdit::multiline(&mut state.paste_buffer)
                                .desired_width(f32::INFINITY)
                                .desired_rows(10)
                                .font(egui::TextStyle::Monospace),
                        );
                    });
                ui.horizontal(|ui| {
                    if ui.button("Equip from paste").clicked() {
                        match parse_item(&state.paste_buffer) {
                            Ok(item) => {
                                items.equip(slot, item);
                                state.last_error = None;
                                state.paste_buffer.clear();
                                changed = true;
                            }
                            Err(e) => {
                                state.last_error = Some(e.to_string());
                            }
                        }
                    }
                    if ui
                        .button("Auto-equip (detect slot)")
                        .on_hover_text(
                            "Parse the pasted item and equip it to whichever \
                             slot its `Item Class:` line maps to (e.g. amulets \
                             → Amulet slot). When the swap pair is live, \
                             detected weapons go to the swap slots so the \
                             paste targets the visible/active pair.",
                        )
                        .clicked()
                    {
                        match parse_item(&state.paste_buffer) {
                            Ok(item) => {
                                let detected = detect_slot(&item.base_name)
                                    .or_else(|| detect_slot_from_class(&state.paste_buffer));
                                // Issue #109 (slice 4): when the swap pair
                                // is live, detected weapons should target
                                // the swap slots — that's the pair the
                                // user is staring at and wants to fill.
                                let detected = detected.map(|s| {
                                    if use_swap {
                                        match s {
                                            Slot::Weapon1 => Slot::Weapon1Swap,
                                            Slot::Weapon2 => Slot::Weapon2Swap,
                                            other => other,
                                        }
                                    } else {
                                        s
                                    }
                                });
                                if let Some(target) = detected {
                                    items.equip(target, item);
                                    state.selected_slot = Some(target);
                                    state.last_error = None;
                                    state.paste_buffer.clear();
                                    changed = true;
                                } else {
                                    state.last_error = Some(
                                        "Could not detect the right slot — \
                                         use \"Equip from paste\" with a \
                                         specific slot selected."
                                            .into(),
                                    );
                                }
                            }
                            Err(e) => {
                                state.last_error = Some(e.to_string());
                            }
                        }
                    }
                    if ui.button("Clear paste").clicked() {
                        state.paste_buffer.clear();
                        state.last_error = None;
                    }
                });
                if let Some(err) = &state.last_error {
                    ui.colored_label(egui::Color32::LIGHT_RED, err);
                }
            } else {
                ui.label("Pick a slot on the left.");
            }
        });
    });
    changed
}

/// Detect the equipment slot from the base name (e.g. "Onyx Amulet" → Amulet).
/// Returns None if the base type doesn't map to a single slot — e.g. rings could
/// fit either Ring1 or Ring2 (caller's responsibility to disambiguate).
fn detect_slot(base_name: &str) -> Option<Slot> {
    let lower = base_name.to_lowercase();
    if lower.ends_with("amulet") || lower.contains("talisman") {
        return Some(Slot::Amulet);
    }
    if lower.ends_with("belt") || lower.contains("sash") || lower.contains("girdle") {
        return Some(Slot::Belt);
    }
    if lower.contains("ring") {
        return Some(Slot::Ring1);
    }
    if lower.contains("flask") {
        return Some(Slot::Flask1);
    }
    None
}

/// Map "Item Class: X" lines that PoE pastes include to the corresponding slot.
fn detect_slot_from_class(raw: &str) -> Option<Slot> {
    let line = raw
        .lines()
        .find(|l| l.trim_start().starts_with("Item Class:"))?
        .split_once(':')?
        .1
        .trim()
        .to_lowercase();
    Some(match line.as_str() {
        "amulets" => Slot::Amulet,
        "rings" => Slot::Ring1,
        "belts" => Slot::Belt,
        "helmets" => Slot::Helmet,
        "body armours" => Slot::BodyArmour,
        "gloves" => Slot::Gloves,
        "boots" => Slot::Boots,
        "quivers" => Slot::Weapon2,
        s if s.contains("flask") => Slot::Flask1,
        s if s.contains("axes")
            || s.contains("swords")
            || s.contains("maces")
            || s.contains("daggers")
            || s.contains("claws")
            || s.contains("staves")
            || s.contains("bows")
            || s.contains("wands")
            || s.contains("sceptres")
            || s.contains("spears") =>
        {
            Slot::Weapon1
        }
        s if s.contains("shield") => Slot::Weapon2,
        _ => return None,
    })
}

fn rarity_glyph(r: Rarity) -> &'static str {
    match r {
        Rarity::Normal => "·",
        Rarity::Magic => "M",
        Rarity::Rare => "R",
        Rarity::Unique => "U",
        Rarity::Relic => "L",
    }
}

fn render_item_summary(ui: &mut egui::Ui, item: &Item) {
    let body_font = egui::TextStyle::Body.resolve(ui.style());
    let strong_font = body_font.clone();
    // Item name — render with PoB color escapes if present (e.g. unique
    // names use `^xRRGGBB`); fall back to default text colour otherwise.
    let name_default = ui.style().visuals.strong_text_color();
    let name_job = color_codes::to_layout_job(&item.name, name_default, strong_font);
    ui.label(name_job);
    ui.label(&item.base_name);
    ui.label(format!(
        "{:?} • iLvl {} • Q{}{}",
        item.rarity,
        item.item_level,
        item.quality,
        if item.corrupted { " • Corrupted" } else { "" }
    ));
    if !item.sockets.is_empty() {
        ui.label(format!("Sockets: {}", item.sockets));
    }
    ui.add_space(4.0);
    for ml in &item.mod_lines {
        let section_default = match ml.section {
            pob_data::ModSection::Implicit => egui::Color32::from_rgb(200, 200, 255),
            pob_data::ModSection::Crafted => egui::Color32::from_rgb(180, 230, 255),
            pob_data::ModSection::Enchant => egui::Color32::from_rgb(180, 230, 180),
            pob_data::ModSection::Fractured => egui::Color32::from_rgb(220, 200, 130),
            pob_data::ModSection::Corrupted => egui::Color32::from_rgb(220, 100, 220),
            pob_data::ModSection::Veiled => egui::Color32::from_rgb(180, 180, 180),
            pob_data::ModSection::Explicit => egui::Color32::from_rgb(220, 220, 100),
        };
        // Inline `^N` / `^xRRGGBB` escapes in the mod line override the
        // section colour for the runs they cover; sections without any
        // escape render in the section's default colour as before.
        let job = color_codes::to_layout_job(&ml.line, section_default, body_font.clone());
        ui.label(job);
    }
}
