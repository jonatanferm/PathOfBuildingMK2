//! Items tab — slot grid + paste-to-equip + browseable base catalogue.

use std::cmp::Ordering;

use eframe::egui;
use pob_data::bases::ItemBaseSet;
use pob_data::{Item, ItemBase, ItemSet, ModLine, ModSection, Rarity, Slot};
use pob_engine::{parse_item, Character};

use crate::color_codes;
use crate::shared_items::{SharedItem, SharedItemStore};
use crate::sortable_list::{
    column_header, cycle_sort, sorted_indices, text_filter_matches, SortState,
};

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
    /// Issue #211: persistent sort state for the browse panel's column
    /// headers. `None` means "natural (alphabetical) order" — what the
    /// panel showed before this issue. Lives on the tab state so the
    /// chosen sort survives tab switches in the session.
    pub browse_sort: Option<SortState<BrowseColumn>>,
    /// Issue #209: which sub-list the browse panel is currently
    /// showing — the bundled item-base catalogue or the user's saved
    /// shared items. Mirrors PoB's two-tab `ItemDBControl` /
    /// `SharedItemListControl` split.
    pub browse_view: BrowseView,
    /// Issue #209: buffer for the "Save current item as shared" label input.
    pub new_shared_label: String,
}

/// Issue #209: which sub-list the browse panel is currently showing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum BrowseView {
    /// The bundled item-base catalogue (`bases.json`).
    #[default]
    Bases,
    /// The user's saved shared items (persisted across restarts).
    Shared,
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
            browse_sort: None,
            browse_view: BrowseView::default(),
            new_shared_label: String::new(),
        }
    }
}

/// Filter predicate inputs for the base browser. Defaults match every base.
///
/// Issue #211 added the per-column `name_filter` / `class_filter` text
/// inputs alongside the existing global `search` box. They're additive:
/// a base passes only if it satisfies the slot filter, the global
/// search, and *both* per-column filters. Empty fields are no-ops so
/// the prior behaviour is preserved when the user ignores the new row.
///
/// Issue #209 added the `rarity` filter — meaningful for the shared
/// items list (each saved row carries the rarity it was saved at) and
/// excludes the base catalogue when the user picks a non-Normal
/// rarity (bases conceptually roll Normal).
#[derive(Debug, Clone, Default)]
pub struct BrowseFilter {
    pub slot: Option<BrowseSlot>,
    pub search: String,
    pub name_filter: String,
    pub class_filter: String,
    pub rarity: Option<Rarity>,
}

/// Columns the browse panel exposes for sorting (and filtering, per
/// Issue #211). The full list of upstream PoB columns (DPS contribution,
/// level, etc.) is wider; we ship the name + class pair the panel
/// already renders in this slice and leave the rest as follow-up.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BrowseColumn {
    Name,
    Class,
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

    /// Issue #209: shared-items list rows don't carry a base `type`
    /// field, only the in-game base name (e.g. "Onyx Amulet"). Use a
    /// distinct heuristic that looks at name suffixes / keywords so
    /// the slot filter still narrows the saved list correctly.
    pub fn from_base_name(name: &str) -> Self {
        let lower = name.to_ascii_lowercase();
        if lower.ends_with("amulet") || lower.contains("talisman") {
            Self::Amulet
        } else if lower.ends_with("ring") {
            Self::Ring
        } else if lower.ends_with("belt") || lower.contains("sash") || lower.contains("girdle") {
            Self::Belt
        } else if lower.contains("flask") || lower.contains("tincture") {
            Self::Flask
        } else if lower.contains("quiver") {
            Self::Quiver
        } else if lower.contains("shield") || lower.contains("buckler") || lower.contains("bundle")
        {
            Self::Shield
        } else if lower.contains("jewel") {
            Self::Jewel
        } else if lower.ends_with("boots")
            || lower.ends_with("greaves")
            || lower.ends_with("slippers")
            || lower.ends_with("shoes")
        {
            Self::Boots
        } else if lower.ends_with("gloves")
            || lower.ends_with("gauntlets")
            || lower.ends_with("mitts")
            || lower.ends_with("bracers")
        {
            Self::Gloves
        } else if lower.ends_with("helmet")
            || lower.ends_with("helm")
            || lower.ends_with("cap")
            || lower.ends_with("hood")
            || lower.ends_with("burgonet")
            || lower.ends_with("crown")
            || lower.ends_with("circlet")
            || lower.ends_with("hat")
            || lower.ends_with("mask")
            || lower.ends_with("tricorne")
            || lower.ends_with("bascinet")
        {
            Self::Helmet
        } else if lower.contains("vest")
            || lower.contains("plate")
            || lower.contains("garb")
            || lower.contains("robe")
            || lower.contains("jerkin")
            || lower.contains("doublet")
            || lower.contains("brigandine")
            || lower.contains("hauberk")
            || lower.contains("cuirass")
            || lower.contains("raiment")
            || lower.contains("vestments")
        {
            Self::BodyArmour
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
///
/// Issue #211: combines the prior global search with per-column text
/// filters via [`text_filter_matches`]. The slot pill stays as the
/// coarsest filter — class search is *additive* on top.
#[must_use]
pub fn base_matches_filter(name: &str, base: &ItemBase, filter: &BrowseFilter) -> bool {
    if let Some(slot) = filter.slot {
        if BrowseSlot::from_base_type(&base.r#type) != slot {
            return false;
        }
    }
    if let Some(rarity) = filter.rarity {
        // Bases roll Normal by default; any other rarity filter
        // excludes the entire base catalogue (they're crafted /
        // unique items, not raw bases).
        if rarity != Rarity::Normal {
            return false;
        }
    }
    // Global search — name OR class match (preserves prior behaviour).
    if !text_filter_matches(&filter.search, [name, base.r#type.as_str()]) {
        return false;
    }
    // Per-column text filters: each must independently match its target
    // field. This is the user-visible hook for the issue's "per-column
    // filter row" requirement on the items list.
    if !text_filter_matches(&filter.name_filter, [name]) {
        return false;
    }
    if !text_filter_matches(&filter.class_filter, [base.r#type.as_str()]) {
        return false;
    }
    true
}

/// Filter predicate for a saved shared item. Mirrors
/// [`base_matches_filter`] but the rarity check uses the item's own
/// rarity (saved rares stay visible when the user picks "Rare", etc.)
/// and slot mapping goes through the item's `base_name` heuristic
/// since saved items don't carry a base `type` field.
#[must_use]
pub fn shared_matches_filter(saved: &SharedItem, filter: &BrowseFilter) -> bool {
    if let Some(slot) = filter.slot {
        let bucket = BrowseSlot::from_base_name(&saved.item.base_name);
        if bucket != slot {
            return false;
        }
    }
    if let Some(rarity) = filter.rarity {
        if saved.item.rarity != rarity {
            return false;
        }
    }
    // Global search — match label / item name / base name.
    if !text_filter_matches(
        &filter.search,
        [
            saved.label.as_str(),
            saved.item.name.as_str(),
            saved.item.base_name.as_str(),
        ],
    ) {
        return false;
    }
    true
}

/// Compare two browse rows by the given column. Always defines an
/// ascending order; the [`sortable_list`] helper inverts it on demand
/// for descending sorts. Pulled out so the sort behaviour is unit
/// testable without spinning up an egui context.
#[must_use]
pub fn compare_browse_rows(
    a: (&str, &ItemBase),
    b: (&str, &ItemBase),
    column: BrowseColumn,
) -> Ordering {
    match column {
        BrowseColumn::Name => {
            a.0.to_ascii_lowercase()
                .cmp(&b.0.to_ascii_lowercase())
                // Tie-break on the class column so otherwise-equal names
                // sort stably (e.g. Iron Hat helm vs Iron Hat dummy).
                .then_with(|| a.1.r#type.cmp(&b.1.r#type))
        }
        BrowseColumn::Class => {
            a.1.r#type
                .to_ascii_lowercase()
                .cmp(&b.1.r#type.to_ascii_lowercase())
                .then_with(|| a.0.cmp(b.0))
        }
    }
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
    shared_items: &mut SharedItemStore,
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
                    // Issue #203 (slice 2): rich item-card hover.
                    // Build the tooltip lines per hover frame — cheap
                    // (a few clones / formats) and avoids caching that
                    // would have to invalidate on every item edit.
                    let response = if let Some(item) = equipped {
                        let lines = item_tooltip_lines(item);
                        response.on_hover_ui(|ui| {
                            for line in &lines {
                                ui.label(line);
                            }
                        })
                    } else {
                        response
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
                // Snapshot the equipped item once so the closures below
                // can capture it independently of `items` and
                // `shared_items` mutating borrows.
                let equipped: Option<Item> = items.get(slot).cloned();
                if let Some(item) = equipped {
                    egui::ScrollArea::vertical()
                        .max_height(220.0)
                        .show(ui, |ui| {
                            render_item_summary(ui, &item);
                        });
                    ui.add_space(4.0);
                    let mut do_unequip = false;
                    let mut do_save_shared = false;
                    ui.horizontal(|ui| {
                        if ui.button("Unequip").clicked() {
                            do_unequip = true;
                        }
                        // Issue #209: snapshot the equipped item into
                        // the user-global shared-item store so it
                        // survives across builds + app restarts. The
                        // label input falls back to the item's own
                        // name when blank; auto-dedup handles repeat
                        // saves under the same label.
                        ui.add(
                            egui::TextEdit::singleline(&mut state.new_shared_label)
                                .desired_width(140.0)
                                .hint_text("Save label"),
                        );
                        if ui
                            .button("Save to shared")
                            .on_hover_text(
                                "Save this item into your user-global shared list. \
                                 Persists across app restarts.",
                            )
                            .clicked()
                        {
                            do_save_shared = true;
                        }
                    });
                    if do_unequip {
                        items.unequip(slot);
                        changed = true;
                    }
                    if do_save_shared {
                        shared_items.add(state.new_shared_label.clone(), item);
                        state.new_shared_label.clear();
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
            ui.vertical(|ui| {
                ui.set_min_width(280.0);
                // Issue #209: Bases / Shared tab toggle. Switches the
                // panel between the bundled base catalogue and the
                // user's saved shared items (which persist across
                // restarts). Mirrors PoB's `ItemDBControl` /
                // `SharedItemListControl` two-pane split.
                ui.horizontal(|ui| {
                    ui.selectable_value(&mut state.browse_view, BrowseView::Bases, "Bases");
                    let shared_label = if shared_items.is_empty() {
                        "Shared".to_owned()
                    } else {
                        format!("Shared ({})", shared_items.len())
                    };
                    ui.selectable_value(&mut state.browse_view, BrowseView::Shared, shared_label);
                });
                // Issue #209: shared filter row — search + slot pills +
                // rarity pills. Rendered above the view body so the user's
                // active filters carry across the Bases / Shared toggle.
                render_filter_row(ui, &mut state.browse_filter);
                ui.separator();
                match state.browse_view {
                    BrowseView::Bases => {
                        if let Some(set) = bases {
                            if render_browse_panel(
                                ui,
                                &mut state.browse_filter,
                                &mut state.browse_sort,
                                set,
                                items,
                                &mut state.selected_slot,
                                use_swap,
                            ) {
                                changed = true;
                            }
                        } else {
                            ui.colored_label(
                                egui::Color32::LIGHT_RED,
                                "No item-base data loaded. Re-run \
                                 `cargo run -p pob-extract --release` from the \
                                 workspace root to populate `data/bases.json`.",
                            );
                        }
                    }
                    BrowseView::Shared => {
                        if render_shared_panel(
                            ui,
                            &state.browse_filter,
                            shared_items,
                            items,
                            &mut state.selected_slot,
                            use_swap,
                        ) {
                            changed = true;
                        }
                    }
                }
            });
        }
    });
    changed
}

/// Issue #209: render the shared-items sub-list inside the Browse panel.
/// Each row shows the user's label + base name with a Delete button;
/// double-click equips into the slot detected from the saved item's
/// base name (or the user's currently-selected slot as a fallback),
/// mirroring the base-browser flow. Filter is applied via
/// [`shared_matches_filter`] so search / slot / rarity pills carry
/// over from the Bases view.
fn render_shared_panel(
    ui: &mut egui::Ui,
    filter: &BrowseFilter,
    shared_items: &mut SharedItemStore,
    items: &mut ItemSet,
    selected_slot: &mut Option<Slot>,
    use_swap: bool,
) -> bool {
    let mut changed = false;
    if shared_items.is_empty() {
        ui.weak(
            "No shared items yet. Use \"Save to shared\" on an equipped \
             item to add one — saves persist across app restarts.",
        );
        return changed;
    }
    let total = shared_items.len();
    let mut filtered: Vec<usize> = (0..total)
        .filter(|i| shared_matches_filter(&shared_items.items[*i], filter))
        .collect();
    filtered.sort_by(|a, b| {
        shared_items.items[*a]
            .label
            .to_ascii_lowercase()
            .cmp(&shared_items.items[*b].label.to_ascii_lowercase())
    });
    ui.label(format!("{} of {} saved", filtered.len(), total));
    ui.weak("Double-click to equip into the matching slot.");
    ui.add_space(2.0);

    let mut to_delete: Option<usize> = None;
    let mut to_equip: Option<(Slot, Item)> = None;
    egui::ScrollArea::vertical()
        .max_height(420.0)
        .show(ui, |ui| {
            for idx in filtered {
                let saved = &shared_items.items[idx];
                let target = detect_slot(&saved.item.base_name)
                    .or_else(|| detect_slot_from_class(&saved.item.raw))
                    .map(|s| match (s, use_swap) {
                        (Slot::Weapon1, true) => Slot::Weapon1Swap,
                        (Slot::Weapon2, true) => Slot::Weapon2Swap,
                        (other, _) => other,
                    });
                let target_label = target
                    .map(|s| s.label().to_owned())
                    .unwrap_or_else(|| "(pick slot)".to_owned());
                let glyph = rarity_glyph(saved.item.rarity);
                let display_name = if saved.item.name.is_empty() {
                    saved.item.base_name.as_str()
                } else {
                    saved.item.name.as_str()
                };
                ui.horizontal(|ui| {
                    let label_text = format!(
                        "{glyph} {label}\n    {name}  →  {target_label}",
                        label = saved.label,
                        name = display_name,
                    );
                    // Issue #203 (slice 2): rich shared-item hover.
                    // Build the lines once per hover frame and append
                    // the action hint so it doesn't require its own
                    // tooltip surface.
                    let mut lines = item_tooltip_lines(&saved.item);
                    lines.push(String::new());
                    lines.push("Double-click to equip this saved item.".into());
                    let row = ui
                        .add(egui::Label::new(label_text).sense(egui::Sense::click()))
                        .on_hover_ui(|ui| {
                            for line in &lines {
                                if line.is_empty() {
                                    ui.add_space(4.0);
                                } else {
                                    ui.label(line);
                                }
                            }
                        });
                    if row.double_clicked() {
                        let dest = target.or(*selected_slot);
                        if let Some(slot) = dest {
                            to_equip = Some((slot, saved.item.clone()));
                        }
                    }
                    if ui
                        .small_button("✕")
                        .on_hover_text(format!("Delete \"{}\" from shared items", saved.label))
                        .clicked()
                    {
                        to_delete = Some(idx);
                    }
                });
            }
        });
    if let Some((slot, item)) = to_equip {
        items.equip(slot, item);
        *selected_slot = Some(slot);
        changed = true;
    }
    if let Some(idx) = to_delete {
        shared_items.remove(idx);
        // Removal mutates the store; the caller's per-frame flush picks
        // it up via `dirty`. We don't return `changed = true` here
        // because no calc-affecting state changed — just the saved-list.
    }
    changed
}

/// Render the right-hand "Browse" panel listing every base in `set`, filtered
/// by `filter`. Returns true if a base was double-clicked into a slot.
///
/// Issue #211: column headers (Name / Class) are clickable to sort and
/// the row above the list exposes a per-column text filter. Sort state
/// is owned by the caller (see [`ItemsTabState::browse_sort`]) so it
/// persists across tab switches in the session.
/// Issue #209: render the search + slot + rarity filter pills shared
/// between the Bases and Shared sub-views. Pulled out so the filter
/// state is unified — switching between views preserves the user's
/// active filters.
fn render_filter_row(ui: &mut egui::Ui, filter: &mut BrowseFilter) {
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

    // Issue #209: rarity pill row. Mirrors PoB's
    // `ItemDBControl.lua` rarity dropdown — fixed buckets matching
    // the in-game rarities. Rendered as selectable pills to fit
    // egui's idiom (consistent with the slot row above).
    ui.horizontal_wrapped(|ui| {
        ui.label("Rarity:");
        if ui
            .selectable_label(filter.rarity.is_none(), "Any")
            .clicked()
        {
            filter.rarity = None;
        }
        for r in [
            Rarity::Normal,
            Rarity::Magic,
            Rarity::Rare,
            Rarity::Unique,
            Rarity::Relic,
        ] {
            let active = filter.rarity == Some(r);
            let label = match r {
                Rarity::Normal => "Normal",
                Rarity::Magic => "Magic",
                Rarity::Rare => "Rare",
                Rarity::Unique => "Unique",
                Rarity::Relic => "Relic",
            };
            if ui.selectable_label(active, label).clicked() {
                filter.rarity = if active { None } else { Some(r) };
            }
        }
    });
}

fn render_browse_panel(
    ui: &mut egui::Ui,
    filter: &mut BrowseFilter,
    sort: &mut Option<SortState<BrowseColumn>>,
    set: &ItemBaseSet,
    items: &mut ItemSet,
    selected_slot: &mut Option<Slot>,
    use_swap: bool,
) -> bool {
    let mut changed = false;
    ui.vertical(|ui| {
        ui.set_min_width(320.0);
        ui.heading("Browse bases");
        ui.separator();

        // Issue #211: clickable column headers + per-column text filters.
        // The header row mirrors PoB's `ListControl.lua` layout — header
        // cell on top, filter input on the bottom row.
        ui.horizontal(|ui| {
            if column_header(ui, "Name", BrowseColumn::Name, *sort) {
                *sort = cycle_sort(*sort, BrowseColumn::Name);
            }
            ui.add_space(140.0);
            if column_header(ui, "Class", BrowseColumn::Class, *sort) {
                *sort = cycle_sort(*sort, BrowseColumn::Class);
            }
        });
        ui.horizontal(|ui| {
            ui.add(
                egui::TextEdit::singleline(&mut filter.name_filter)
                    .hint_text("filter name…")
                    .desired_width(160.0),
            );
            ui.add(
                egui::TextEdit::singleline(&mut filter.class_filter)
                    .hint_text("filter class…")
                    .desired_width(140.0),
            );
            // Single "clear all column filters" exit hatch — keeps the
            // header row tidy when the user wants to revert.
            if ui
                .small_button("×")
                .on_hover_text("Clear column filters")
                .clicked()
            {
                filter.name_filter.clear();
                filter.class_filter.clear();
            }
        });

        ui.separator();

        // Pre-filter into a sortable Vec so we can show "X of Y" and
        // avoid re-walking the IndexMap during scroll-area culling.
        let mut rows: Vec<(&String, &ItemBase)> = set
            .iter()
            .filter(|(name, base)| base_matches_filter(name, base, filter))
            .collect();
        // Apply the sort via the shared helper. When `sort` is `None`
        // we keep the prior alphabetical default so the panel doesn't
        // change shape until the user clicks a header.
        let order = sorted_indices(&rows, *sort, |a, b, col| {
            compare_browse_rows((a.0.as_str(), a.1), (b.0.as_str(), b.1), col)
        });
        let permuted: Vec<(&String, &ItemBase)> = if sort.is_some() {
            order.iter().map(|&i| rows[i]).collect()
        } else {
            rows.sort_by(|a, b| a.0.cmp(b.0));
            rows
        };

        ui.label(format!("{} of {} bases", permuted.len(), set.len()));
        ui.weak("Double-click to equip a Normal-rarity copy.");
        ui.add_space(2.0);

        egui::ScrollArea::vertical().show(ui, |ui| {
            for (name, base) in permuted {
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

/// Issue #203 (slice 2): build the body of the item-card hover
/// tooltip. Mirrors the editor-panel layout (`render_item_summary`)
/// but as a flat `Vec<String>` so it's pure / unit-testable. Each
/// entry is one rendered line; an empty string is a visual spacer.
///
/// Section ordering follows PoB's `Item:BuildAndParseRaw` output:
/// Enchant → Implicit → Explicit → Fractured → Crafted → Veiled →
/// Corrupted. We skip empty sections so a basic rare doesn't render
/// blank section dividers.
pub fn item_tooltip_lines(item: &Item) -> Vec<String> {
    let mut out = Vec::new();
    if item.name.is_empty() {
        out.push(item.base_name.clone());
    } else {
        out.push(item.name.clone());
        out.push(item.base_name.clone());
    }
    if item.quality > 0 {
        out.push(format!("Quality: +{}%", item.quality));
    }
    if item.item_level > 0 {
        out.push(format!("Item Level: {}", item.item_level));
    }
    // Sections rendered in PoB order; we walk this list and emit each
    // section that has at least one matching mod line.
    const ORDERED: &[(ModSection, &str)] = &[
        (ModSection::Enchant, "Enchant"),
        (ModSection::Implicit, "Implicit"),
        (ModSection::Explicit, "Explicit"),
        (ModSection::Fractured, "Fractured"),
        (ModSection::Crafted, "Crafted"),
        (ModSection::Veiled, "Veiled"),
        (ModSection::Corrupted, "Corrupted"),
    ];
    for (section, label) in ORDERED {
        let mut any = false;
        for ml in &item.mod_lines {
            if ml.section == *section {
                if !any {
                    out.push(format!("--- {label} ---"));
                    any = true;
                }
                out.push(ml.line.clone());
            }
        }
    }
    if item.corrupted {
        out.push("Corrupted".into());
    }
    if item.mirrored {
        out.push("Mirrored".into());
    }
    out
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
            ..Default::default()
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
            search: "rusted".into(),
            ..Default::default()
        };
        assert!(base_matches_filter("Rusted Sword", &base, &filter));
        assert!(!base_matches_filter("Iron Sword", &base, &filter));

        // Empty search matches everything.
        let empty = BrowseFilter::default();
        assert!(base_matches_filter("Anything", &base, &empty));

        // Class match — search hits the `type` field.
        let class_filter = BrowseFilter {
            search: "one handed".into(),
            ..Default::default()
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

    fn mk_item(name: &str, base_name: &str, rarity: Rarity, mods: &[(ModSection, &str)]) -> Item {
        Item {
            name: name.to_owned(),
            base_name: base_name.to_owned(),
            rarity,
            item_level: 0,
            quality: 0,
            tags: HashSet::default(),
            mod_lines: mods
                .iter()
                .map(|(section, line)| ModLine {
                    section: *section,
                    line: (*line).to_owned(),
                })
                .collect(),
            sockets: String::new(),
            raw: String::new(),
            corrupted: false,
            mirrored: false,
        }
    }

    #[test]
    fn item_tooltip_header_shows_name_then_base_for_named_rare() {
        // Issue #203 (slice 2): item-card hover. The user reads the
        // name first, then needs the base type to know what mod pool
        // applies. We mirror PoB's two-line header.
        let item = mk_item(
            "Doom Howl",
            "Vaal Regalia",
            Rarity::Rare,
            &[(ModSection::Explicit, "+100 to maximum Life")],
        );
        let lines = item_tooltip_lines(&item);
        assert_eq!(lines.first().map(String::as_str), Some("Doom Howl"));
        assert_eq!(lines.get(1).map(String::as_str), Some("Vaal Regalia"));
    }

    #[test]
    fn item_tooltip_header_uses_base_name_when_unnamed() {
        // Normal / unidentified items have empty `name`; we shouldn't
        // render a blank first line.
        let item = mk_item(
            "",
            "Iron Ring",
            Rarity::Normal,
            &[(ModSection::Implicit, "+5 to Strength")],
        );
        let lines = item_tooltip_lines(&item);
        assert_eq!(lines.first().map(String::as_str), Some("Iron Ring"));
        // Second line shouldn't repeat the base when the header
        // already used it.
        assert_ne!(lines.get(1).map(String::as_str), Some("Iron Ring"));
    }

    #[test]
    fn item_tooltip_groups_mods_by_section_in_pob_order() {
        // PoB renders sections in fixed order regardless of paste
        // order: Enchant, Implicit, Explicit, Fractured, Crafted,
        // Veiled, Corrupted. Each section gets a `--- <Name> ---`
        // divider. We verify ordering by feeding mods in *reversed*
        // order — the formatter must reorder them.
        let item = mk_item(
            "Doom Howl",
            "Vaal Regalia",
            Rarity::Rare,
            &[
                (ModSection::Crafted, "+30 to maximum Mana"),
                (ModSection::Explicit, "+100 to maximum Life"),
                (ModSection::Implicit, "8% increased maximum Energy Shield"),
                (ModSection::Enchant, "Increased Damage"),
            ],
        );
        let lines = item_tooltip_lines(&item);
        // Header lines come first (name + base), then sections in PoB
        // order. Find each marker and assert ordering.
        let pos = |needle: &str| lines.iter().position(|l| l == needle);
        let enchant = pos("--- Enchant ---").expect("missing enchant divider");
        let implicit = pos("--- Implicit ---").expect("missing implicit divider");
        let explicit = pos("--- Explicit ---").expect("missing explicit divider");
        let crafted = pos("--- Crafted ---").expect("missing crafted divider");
        assert!(
            enchant < implicit && implicit < explicit && explicit < crafted,
            "section ordering wrong: enchant={enchant}, implicit={implicit}, explicit={explicit}, crafted={crafted} ({lines:?})"
        );
        // Each mod line appears below its section divider.
        assert!(lines.contains(&"Increased Damage".to_owned()));
        assert!(lines.contains(&"+100 to maximum Life".to_owned()));
        assert!(lines.contains(&"8% increased maximum Energy Shield".to_owned()));
        assert!(lines.contains(&"+30 to maximum Mana".to_owned()));
    }

    #[test]
    fn item_tooltip_omits_empty_section_dividers() {
        // A basic rare with only explicit mods shouldn't render
        // `--- Enchant ---` blank section markers — that's just noise.
        let item = mk_item(
            "Boring Rare",
            "Iron Ring",
            Rarity::Rare,
            &[(ModSection::Explicit, "+5 to all Attributes")],
        );
        let lines = item_tooltip_lines(&item);
        assert!(
            !lines.iter().any(|l| l == "--- Enchant ---"),
            "spurious enchant divider in {lines:?}"
        );
        assert!(
            !lines.iter().any(|l| l == "--- Implicit ---"),
            "spurious implicit divider in {lines:?}"
        );
        assert!(
            lines.iter().any(|l| l == "--- Explicit ---"),
            "missing explicit divider in {lines:?}"
        );
    }

    #[test]
    fn item_tooltip_surfaces_quality_and_item_level_when_set() {
        // Quality and ilvl matter for crafting / corruption decisions.
        // Show them only when non-zero so basic items stay tidy.
        let mut item = mk_item(
            "Quality Boots",
            "Wool Shoes",
            Rarity::Magic,
            &[(ModSection::Explicit, "+15% Movement Speed")],
        );
        let plain = item_tooltip_lines(&item);
        assert!(
            !plain.iter().any(|l| l.starts_with("Quality:")),
            "spurious quality line in {plain:?}"
        );
        assert!(
            !plain.iter().any(|l| l.starts_with("Item Level:")),
            "spurious ilvl line in {plain:?}"
        );

        item.quality = 20;
        item.item_level = 84;
        let lines = item_tooltip_lines(&item);
        assert!(
            lines.iter().any(|l| l == "Quality: +20%"),
            "missing quality line in {lines:?}"
        );
        assert!(
            lines.iter().any(|l| l == "Item Level: 84"),
            "missing ilvl line in {lines:?}"
        );
    }

    #[test]
    fn item_tooltip_corrupted_marker_appears_when_corrupted() {
        let mut item = mk_item(
            "Doom Howl",
            "Vaal Regalia",
            Rarity::Rare,
            &[(ModSection::Explicit, "+100 to maximum Life")],
        );
        let plain_lines = item_tooltip_lines(&item);
        assert!(
            !plain_lines.iter().any(|l| l == "Corrupted"),
            "spurious corrupted marker in {plain_lines:?}"
        );
        item.corrupted = true;
        let lines = item_tooltip_lines(&item);
        assert!(
            lines.iter().any(|l| l == "Corrupted"),
            "missing corrupted marker in {lines:?}"
        );
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
    fn per_column_filters_narrow_results_independently() {
        let amulet = make_base("Amulet", None, None);
        let helmet = make_base("Helmet", Some("Armour"), None);

        // Name column filter — keeps only matching names regardless of class.
        let by_name = BrowseFilter {
            name_filter: "onyx".into(),
            ..Default::default()
        };
        assert!(base_matches_filter("Onyx Amulet", &amulet, &by_name));
        assert!(!base_matches_filter("Iron Hat", &helmet, &by_name));

        // Class column filter — keeps only matching classes.
        let by_class = BrowseFilter {
            class_filter: "helmet".into(),
            ..Default::default()
        };
        assert!(base_matches_filter("Iron Hat", &helmet, &by_class));
        assert!(!base_matches_filter("Onyx Amulet", &amulet, &by_class));

        // Both filters set — result must satisfy both (AND).
        let both = BrowseFilter {
            name_filter: "iron".into(),
            class_filter: "amulet".into(),
            ..Default::default()
        };
        assert!(!base_matches_filter("Iron Hat", &helmet, &both));
        assert!(!base_matches_filter("Onyx Amulet", &amulet, &both));
    }

    #[test]
    fn compare_browse_rows_sorts_by_chosen_column() {
        let amulet = make_base("Amulet", None, None);
        let helmet = make_base("Helmet", Some("Armour"), None);

        // Name asc: "Iron Hat" < "Onyx Amulet" alphabetically.
        let cmp = compare_browse_rows(
            ("Iron Hat", &helmet),
            ("Onyx Amulet", &amulet),
            BrowseColumn::Name,
        );
        assert_eq!(cmp, std::cmp::Ordering::Less);

        // Class asc: "Amulet" < "Helmet".
        let cmp = compare_browse_rows(
            ("Iron Hat", &helmet),
            ("Onyx Amulet", &amulet),
            BrowseColumn::Class,
        );
        assert_eq!(cmp, std::cmp::Ordering::Greater);
    }

    #[test]
    fn sorted_indices_with_browse_rows_orders_by_class() {
        // Build a small Vec of (name, base) tuples and ensure the
        // shared sort helper orders them by class column ascending.
        let amulet = make_base("Amulet", None, None);
        let helmet = make_base("Helmet", None, None);
        let weapon = make_base("Sword", None, None);
        let rows: Vec<(&str, &ItemBase)> = vec![
            ("Iron Hat", &helmet),
            ("Onyx Amulet", &amulet),
            ("Foo Blade", &weapon),
        ];
        let order = crate::sortable_list::sorted_indices(
            &rows,
            Some(crate::sortable_list::SortState::new(
                BrowseColumn::Class,
                crate::sortable_list::SortDirection::Asc,
            )),
            |a, b, col| compare_browse_rows(*a, *b, col),
        );
        // Amulet (1) < Helmet (0) < Sword (2)
        assert_eq!(order, vec![1, 0, 2]);

        // Descending reverses.
        let order = crate::sortable_list::sorted_indices(
            &rows,
            Some(crate::sortable_list::SortState::new(
                BrowseColumn::Class,
                crate::sortable_list::SortDirection::Desc,
            )),
            |a, b, col| compare_browse_rows(*a, *b, col),
        );
        assert_eq!(order, vec![2, 0, 1]);
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

    fn make_item(name: &str, base: &str, rarity: Rarity) -> Item {
        Item {
            name: name.to_owned(),
            base_name: base.to_owned(),
            rarity,
            item_level: 84,
            quality: 0,
            tags: HashSet::default(),
            mod_lines: Vec::new(),
            sockets: String::new(),
            raw: String::new(),
            corrupted: false,
            mirrored: false,
        }
    }

    #[test]
    fn rarity_filter_excludes_bases_when_non_normal() {
        let base = make_base("Helmet", None, None);
        let normal = BrowseFilter {
            rarity: Some(Rarity::Normal),
            ..Default::default()
        };
        // Bases conceptually roll Normal, so Normal keeps them.
        assert!(base_matches_filter("Iron Hat", &base, &normal));

        for r in [Rarity::Magic, Rarity::Rare, Rarity::Unique, Rarity::Relic] {
            let filter = BrowseFilter {
                rarity: Some(r),
                ..Default::default()
            };
            assert!(
                !base_matches_filter("Iron Hat", &base, &filter),
                "non-Normal rarity ({r:?}) should hide bases"
            );
        }
    }

    #[test]
    fn shared_filter_matches_label_name_or_base_and_rarity() {
        let saved = SharedItem {
            label: "Bossing belt".into(),
            item: make_item("Headhunter", "Leather Belt", Rarity::Unique),
        };

        // Label match.
        assert!(shared_matches_filter(
            &saved,
            &BrowseFilter {
                search: "bossing".into(),
                ..Default::default()
            }
        ));

        // Item-name match (case insensitive).
        assert!(shared_matches_filter(
            &saved,
            &BrowseFilter {
                search: "HEADHUNTER".into(),
                ..Default::default()
            }
        ));

        // Base-name match.
        assert!(shared_matches_filter(
            &saved,
            &BrowseFilter {
                search: "leather".into(),
                ..Default::default()
            }
        ));

        // Mismatched rarity excludes.
        assert!(!shared_matches_filter(
            &saved,
            &BrowseFilter {
                rarity: Some(Rarity::Rare),
                ..Default::default()
            }
        ));

        // Matching rarity keeps it.
        assert!(shared_matches_filter(
            &saved,
            &BrowseFilter {
                rarity: Some(Rarity::Unique),
                ..Default::default()
            }
        ));

        // Slot bucket via base name.
        assert!(shared_matches_filter(
            &saved,
            &BrowseFilter {
                slot: Some(BrowseSlot::Belt),
                ..Default::default()
            }
        ));
        assert!(!shared_matches_filter(
            &saved,
            &BrowseFilter {
                slot: Some(BrowseSlot::Helmet),
                ..Default::default()
            }
        ));
    }

    #[test]
    fn from_base_name_buckets_common_armours_and_jewellery() {
        assert_eq!(
            BrowseSlot::from_base_name("Onyx Amulet"),
            BrowseSlot::Amulet
        );
        assert_eq!(
            BrowseSlot::from_base_name("Two-Stone Ring"),
            BrowseSlot::Ring
        );
        assert_eq!(
            BrowseSlot::from_base_name("Stygian Vise"),
            BrowseSlot::Other
        );
        assert_eq!(BrowseSlot::from_base_name("Heavy Belt"), BrowseSlot::Belt);
        assert_eq!(
            BrowseSlot::from_base_name("Diamond Flask"),
            BrowseSlot::Flask
        );
        assert_eq!(
            BrowseSlot::from_base_name("Two-Toned Boots"),
            BrowseSlot::Boots
        );
        assert_eq!(
            BrowseSlot::from_base_name("Sorcerer Gloves"),
            BrowseSlot::Gloves
        );
        assert_eq!(
            BrowseSlot::from_base_name("Eternal Burgonet"),
            BrowseSlot::Helmet
        );
        assert_eq!(
            BrowseSlot::from_base_name("Astral Plate"),
            BrowseSlot::BodyArmour
        );
        assert_eq!(
            BrowseSlot::from_base_name("Imperial Bow"),
            BrowseSlot::Weapon
        );
        assert_eq!(
            BrowseSlot::from_base_name("Rotfeather Talisman"),
            BrowseSlot::Amulet
        );
        assert_eq!(
            BrowseSlot::from_base_name("Cobalt Jewel"),
            BrowseSlot::Jewel
        );
    }
}
