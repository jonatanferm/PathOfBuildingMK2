//! Items tab — slot grid + paste-to-equip + browseable base catalogue.

use eframe::egui;
use pob_data::bases::ItemBaseSet;
use pob_data::{Item, ItemBase, ItemSet, ModLine, ModSection, Rarity, Slot};
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
    /// Whether the "Browse" panel is open. Mirrors PoB's `ItemDBControl` toggle —
    /// when true we render a side panel listing every item base from the
    /// dataset, filterable by slot bucket and search text.
    pub browse_open: bool,
    /// Browse-panel filter state.
    pub browse_filter: BrowseFilter,
}

impl Default for ItemsTabState {
    fn default() -> Self {
        Self {
            selected_slot: Some(Slot::Amulet),
            paste_buffer: String::new(),
            last_error: None,
            new_set_name: String::new(),
            browse_open: false,
            browse_filter: BrowseFilter::default(),
        }
    }
}

/// Filter predicate inputs for the base browser. Defaults match every base.
#[derive(Debug, Clone, Default)]
pub struct BrowseFilter {
    pub slot: Option<BrowseSlot>,
    pub search: String,
}

/// Coarse slot bucket the browse panel groups bases under. Aggregates the
/// in-game `type` field — e.g. `One Handed Axe`, `Sceptre`, `Wand` all map to
/// `Weapon` so a single "Show me weapons" filter is intuitive.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BrowseSlot {
    Helmet,
    BodyArmour,
    Gloves,
    Boots,
    Weapon,
    Shield,
    Ring,
    Amulet,
    Belt,
    Quiver,
    Flask,
    Jewel,
    Other,
}

impl BrowseSlot {
    pub fn all() -> &'static [Self] {
        &[
            Self::Helmet,
            Self::BodyArmour,
            Self::Gloves,
            Self::Boots,
            Self::Weapon,
            Self::Shield,
            Self::Ring,
            Self::Amulet,
            Self::Belt,
            Self::Quiver,
            Self::Flask,
            Self::Jewel,
            Self::Other,
        ]
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Helmet => "Helmet",
            Self::BodyArmour => "Body",
            Self::Gloves => "Gloves",
            Self::Boots => "Boots",
            Self::Weapon => "Weapon",
            Self::Shield => "Shield",
            Self::Ring => "Ring",
            Self::Amulet => "Amulet",
            Self::Belt => "Belt",
            Self::Quiver => "Quiver",
            Self::Flask => "Flask",
            Self::Jewel => "Jewel",
            Self::Other => "Other",
        }
    }

    /// Heuristic mapping from a base's `type` field to a coarse browse bucket.
    pub fn from_base_type(t: &str) -> Self {
        let lower = t.to_ascii_lowercase();
        if lower.contains("helmet") {
            Self::Helmet
        } else if lower.contains("body armour") {
            Self::BodyArmour
        } else if lower.contains("gloves") {
            Self::Gloves
        } else if lower.contains("boots") {
            Self::Boots
        } else if lower.contains("shield") {
            Self::Shield
        } else if lower.contains("quiver") {
            Self::Quiver
        } else if lower.contains("ring") {
            Self::Ring
        } else if lower.contains("amulet") {
            Self::Amulet
        } else if lower.contains("belt") {
            Self::Belt
        } else if lower.contains("flask") || lower.contains("tincture") {
            Self::Flask
        } else if lower.contains("jewel") {
            Self::Jewel
        } else if lower.contains("axe")
            || lower.contains("sword")
            || lower.contains("mace")
            || lower.contains("dagger")
            || lower.contains("claw")
            || lower.contains("staff")
            || lower.contains("staves")
            || lower.contains("bow")
            || lower.contains("wand")
            || lower.contains("sceptre")
            || lower.contains("spear")
            || lower.contains("rod")
        {
            Self::Weapon
        } else {
            Self::Other
        }
    }
}

/// Returns true if a given base passes the current filter. Pulled out so the
/// unit tests can exercise it without an egui context.
#[must_use]
pub fn base_matches_filter(name: &str, base: &ItemBase, filter: &BrowseFilter) -> bool {
    if let Some(slot) = filter.slot {
        if BrowseSlot::from_base_type(&base.r#type) != slot {
            return false;
        }
    }
    let q = filter.search.trim();
    if !q.is_empty() {
        let q_lower = q.to_ascii_lowercase();
        let name_match = name.to_ascii_lowercase().contains(&q_lower);
        let type_match = base.r#type.to_ascii_lowercase().contains(&q_lower);
        if !name_match && !type_match {
            return false;
        }
    }
    true
}

/// Equip slot a freshly-rolled `base` should drop into. Mirrors the PoB
/// `Item:GetTargetSlot` mapping. Returns `None` for jewels / fishing rods /
/// other bases that don't have an obvious single equip slot — caller falls
/// back to the user-selected slot in that case.
#[must_use]
pub fn target_slot_for_base(base: &ItemBase) -> Option<Slot> {
    let lower = base.r#type.to_ascii_lowercase();
    if lower.contains("helmet") {
        Some(Slot::Helmet)
    } else if lower.contains("body armour") {
        Some(Slot::BodyArmour)
    } else if lower.contains("gloves") {
        Some(Slot::Gloves)
    } else if lower.contains("boots") {
        Some(Slot::Boots)
    } else if lower.contains("amulet") {
        Some(Slot::Amulet)
    } else if lower.contains("ring") {
        Some(Slot::Ring1)
    } else if lower.contains("belt") {
        Some(Slot::Belt)
    } else if lower.contains("shield") || lower.contains("quiver") {
        Some(Slot::Weapon2)
    } else if lower.contains("flask") || lower.contains("tincture") {
        Some(Slot::Flask1)
    } else if lower.contains("axe")
        || lower.contains("sword")
        || lower.contains("mace")
        || lower.contains("dagger")
        || lower.contains("claw")
        || lower.contains("staff")
        || lower.contains("bow")
        || lower.contains("wand")
        || lower.contains("sceptre")
        || lower.contains("spear")
        || lower.contains("rod")
    {
        Some(Slot::Weapon1)
    } else {
        None
    }
}

/// Build a minimal Normal-rarity `Item` from a base entry — no explicit mods,
/// implicit copied from the base. This is what the browse panel double-click
/// path equips into a slot.
#[must_use]
pub fn item_from_base(name: &str, base: &ItemBase) -> Item {
    let mut mod_lines = Vec::new();
    if let Some(impl_text) = base.implicit.as_deref() {
        // PoE / PoB-style implicits are sometimes multi-line (e.g. mana mod
        // plus chance mod on the same item). Split so each row reads
        // naturally in the UI.
        for line in impl_text.split('\n') {
            let trimmed = line.trim();
            if trimmed.is_empty() {
                continue;
            }
            mod_lines.push(ModLine {
                line: trimmed.to_owned(),
                section: ModSection::Implicit,
            });
        }
    }
    Item {
        name: String::new(),
        base_name: name.to_owned(),
        rarity: Rarity::Normal,
        item_level: base.req.level.unwrap_or(1),
        quality: 0,
        tags: base.tags.clone(),
        mod_lines,
        sockets: String::new(),
        raw: String::new(),
        corrupted: false,
        mirrored: false,
    }
}

/// Returns true if the equipped items changed (so the caller can recompute).
pub fn ui(
    ui: &mut egui::Ui,
    state: &mut ItemsTabState,
    character: &mut Character,
    bases: Option<&ItemBaseSet>,
) -> bool {
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

            // Browse-panel toggle. Mirrors PoB's "Browse" button on the
            // Items tab that opens an `ItemDBControl` listing every base
            // type. We render the toggle at the bottom of the slot grid
            // so it stays close to the slot list it interacts with.
            ui.add_space(8.0);
            let toggle_label = if state.browse_open {
                "Close browse"
            } else {
                "Browse bases…"
            };
            if ui.button(toggle_label).clicked() {
                state.browse_open = !state.browse_open;
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

        // Far right: optional browse panel.
        if state.browse_open {
            ui.separator();
            if let Some(set) = bases {
                if render_browse_panel(
                    ui,
                    &mut state.browse_filter,
                    set,
                    items,
                    &mut state.selected_slot,
                    use_swap,
                ) {
                    changed = true;
                }
            } else {
                ui.vertical(|ui| {
                    ui.set_min_width(220.0);
                    ui.heading("Browse");
                    ui.separator();
                    ui.colored_label(
                        egui::Color32::LIGHT_RED,
                        "No item-base data loaded. Re-run \
                         `cargo run -p pob-extract --release` from the \
                         workspace root to populate `data/bases.json`.",
                    );
                });
            }
        }
    });
    changed
}

/// Render the right-hand "Browse" panel listing every base in `set`, filtered
/// by `filter`. Returns true if a base was double-clicked into a slot.
fn render_browse_panel(
    ui: &mut egui::Ui,
    filter: &mut BrowseFilter,
    set: &ItemBaseSet,
    items: &mut ItemSet,
    selected_slot: &mut Option<Slot>,
    use_swap: bool,
) -> bool {
    let mut changed = false;
    ui.vertical(|ui| {
        ui.set_min_width(280.0);
        ui.heading("Browse bases");
        ui.separator();

        ui.horizontal(|ui| {
            ui.label("Search:");
            ui.add(
                egui::TextEdit::singleline(&mut filter.search)
                    .hint_text("name or class")
                    .desired_width(160.0),
            );
            if ui.button("×").on_hover_text("Clear search").clicked() {
                filter.search.clear();
            }
        });

        ui.horizontal_wrapped(|ui| {
            // "All" pill resets the slot filter.
            if ui.selectable_label(filter.slot.is_none(), "All").clicked() {
                filter.slot = None;
            }
            for s in BrowseSlot::all() {
                let active = filter.slot == Some(*s);
                if ui.selectable_label(active, s.label()).clicked() {
                    filter.slot = if active { None } else { Some(*s) };
                }
            }
        });

        ui.separator();

        // Pre-filter into a sortable Vec so we can show "X of Y" and avoid
        // re-walking the IndexMap during scroll-area culling.
        let mut rows: Vec<(&String, &ItemBase)> = set
            .iter()
            .filter(|(name, base)| base_matches_filter(name, base, filter))
            .collect();
        rows.sort_by(|a, b| a.0.cmp(b.0));

        ui.label(format!("{} of {} bases", rows.len(), set.len()));
        ui.weak("Double-click to equip a Normal-rarity copy.");
        ui.add_space(2.0);

        egui::ScrollArea::vertical().show(ui, |ui| {
            for (name, base) in rows {
                let target = target_slot_for_base(base);
                // When the swap pair is live, weapons / off-hands target
                // the swap slots so the user fills the visible pair.
                let target = target.map(|s| match (s, use_swap) {
                    (Slot::Weapon1, true) => Slot::Weapon1Swap,
                    (Slot::Weapon2, true) => Slot::Weapon2Swap,
                    (other, _) => other,
                });
                let target_label = target
                    .map(|s| s.label().to_owned())
                    .unwrap_or_else(|| "(pick slot)".to_owned());
                let row = ui
                    .add(
                        egui::Label::new(format!("{name}\n    {}  →  {target_label}", base.r#type))
                            .sense(egui::Sense::click()),
                    )
                    .on_hover_text(
                        "Double-click to equip a fresh Normal-rarity copy of this base.",
                    );
                if row.double_clicked() {
                    let dest = target.or(*selected_slot);
                    if let Some(slot) = dest {
                        let item = item_from_base(name, base);
                        items.equip(slot, item);
                        *selected_slot = Some(slot);
                        changed = true;
                    }
                }
                ui.separator();
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

#[cfg(test)]
mod tests {
    use super::*;
    use ahash::HashSet;
    use pob_data::bases::ItemReq;

    fn make_base(t: &str, sub_type: Option<&str>, implicit: Option<&str>) -> ItemBase {
        ItemBase {
            r#type: t.to_owned(),
            sub_type: sub_type.map(str::to_owned),
            socket_limit: None,
            tags: HashSet::default(),
            influence_tags: indexmap::IndexMap::new(),
            implicit: implicit.map(str::to_owned),
            implicit_mod_types: Vec::new(),
            req: ItemReq::default(),
            weapon: None,
            armour: None,
            flask: None,
        }
    }

    #[test]
    fn slot_filter_keeps_matching_bases() {
        let amulet = make_base("Amulet", None, None);
        let helmet = make_base("Helmet", Some("Armour"), None);

        let mut filter = BrowseFilter {
            slot: Some(BrowseSlot::Amulet),
            search: String::new(),
        };
        assert!(base_matches_filter("Onyx Amulet", &amulet, &filter));
        assert!(!base_matches_filter("Iron Hat", &helmet, &filter));

        filter.slot = Some(BrowseSlot::Helmet);
        assert!(!base_matches_filter("Onyx Amulet", &amulet, &filter));
        assert!(base_matches_filter("Iron Hat", &helmet, &filter));
    }

    #[test]
    fn search_matches_name_or_type_case_insensitive() {
        let base = make_base("One Handed Sword", None, None);
        let filter = BrowseFilter {
            slot: None,
            search: "rusted".into(),
        };
        assert!(base_matches_filter("Rusted Sword", &base, &filter));
        assert!(!base_matches_filter("Iron Sword", &base, &filter));

        // Empty search matches everything.
        let empty = BrowseFilter::default();
        assert!(base_matches_filter("Anything", &base, &empty));

        // Class match — search hits the `type` field.
        let class_filter = BrowseFilter {
            slot: None,
            search: "one handed".into(),
        };
        assert!(base_matches_filter("Whatever", &base, &class_filter));
    }

    #[test]
    fn target_slot_picks_appropriate_slots() {
        let sword = make_base("One Handed Sword", None, None);
        assert_eq!(target_slot_for_base(&sword), Some(Slot::Weapon1));

        let shield = make_base("Shield", Some("Armour"), None);
        assert_eq!(target_slot_for_base(&shield), Some(Slot::Weapon2));

        let ring = make_base("Ring", None, None);
        assert_eq!(target_slot_for_base(&ring), Some(Slot::Ring1));

        let flask = make_base("Flask", Some("Life"), None);
        assert_eq!(target_slot_for_base(&flask), Some(Slot::Flask1));

        let jewel = make_base("Jewel", None, None);
        assert_eq!(target_slot_for_base(&jewel), None);
    }

    #[test]
    fn item_from_base_includes_implicit_lines() {
        let base = make_base(
            "Gloves",
            Some("Energy Shield"),
            Some("30% reduced maximum Mana\n(25-30)% chance when you pay a Skill's Cost"),
        );
        let item = item_from_base("Sorcerer Gloves", &base);
        assert_eq!(item.rarity, Rarity::Normal);
        assert_eq!(item.base_name, "Sorcerer Gloves");
        assert_eq!(item.mod_lines.len(), 2);
        assert!(item
            .mod_lines
            .iter()
            .all(|m| m.section == ModSection::Implicit));

        // No implicit → no mod lines.
        let plain = make_base("Belt", None, None);
        let plain_item = item_from_base("Leather Belt", &plain);
        assert!(plain_item.mod_lines.is_empty());
    }

    #[test]
    fn browse_slot_from_type_handles_weapons_and_armour() {
        assert_eq!(
            BrowseSlot::from_base_type("One Handed Sword"),
            BrowseSlot::Weapon
        );
        assert_eq!(BrowseSlot::from_base_type("Wand"), BrowseSlot::Weapon);
        assert_eq!(
            BrowseSlot::from_base_type("Body Armour"),
            BrowseSlot::BodyArmour
        );
        assert_eq!(BrowseSlot::from_base_type("Helmet"), BrowseSlot::Helmet);
        assert_eq!(BrowseSlot::from_base_type("Quiver"), BrowseSlot::Quiver);
        assert_eq!(BrowseSlot::from_base_type("Jewel"), BrowseSlot::Jewel);
        assert_eq!(BrowseSlot::from_base_type("Tincture"), BrowseSlot::Flask);
    }
}
