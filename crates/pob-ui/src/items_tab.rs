//! Items tab — slot grid + paste-to-equip + browseable base catalogue.

use std::cmp::Ordering;

use eframe::egui;
use pob_data::bases::ItemBaseSet;
use pob_data::{Item, ItemBase, ItemSet, ModLine, ModSection, PassiveTree, Rarity, Slot};
use pob_engine::character::NamedItemSet;
use pob_engine::{
    format_top_contributors, parse_item, rank_item_modlines, Character, ItemModlineScore,
    SkillRegistry,
};

use crate::color_codes;
use crate::shared_items::{SharedItem, SharedItemStore};
use crate::socket_renderer::{draw_sockets, SocketLayoutConfig};
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
    /// Issue #211 (slice 3): pending inline-rename popup state. `Some`
    /// when the user clicked "Rename" on a shared-items row; carries
    /// the row index and the current edit buffer (seeded with the
    /// existing label). Cleared on Save / Cancel / outside-click.
    pub shared_rename: Option<SharedRenameState>,
    /// Issue #212 (slice 1): pending rename-popup state for an *item set*
    /// (not a shared item). `Some` when the user clicked Rename on a saved
    /// item-set chip; carries the set's index, the edit buffer (seeded
    /// with the existing name), and the most recent error to surface
    /// (`DuplicateName`, `EmptyName`) inline so the popup explains why
    /// Save is rejected. Cleared on successful Save / Cancel /
    /// outside-click.
    pub item_set_rename: Option<ItemSetRenameState>,
    /// Issue #207 (panel slice): "Top contributing mod lines" power-report
    /// panel toggle. The panel runs `rank_item_modlines` for every
    /// equipped slot — that's M+1 perform calls per item, so 100+ calls
    /// on a fully-modded build. Off by default; the user opts in by
    /// expanding the collapsing header at the bottom of the tab. Mirrors
    /// PoB's `PowerReportListControl.lua` modline view that shows what's
    /// actually pulling weight on the equipped set.
    pub top_contributors_open: bool,
    /// Issue #207 follow-up: which axis (Combined / DPS / EHP) the
    /// contributors panel sorts by. Defaults to Combined (the historical
    /// `max(dps, ehp)` reducer) so existing builds open the panel with
    /// no behavioural change. Reuses the heatmap's enum because both
    /// reports score on the same dps/ehp delta pair.
    pub top_contributors_axis: crate::node_power_heatmap::HeatmapStat,
    /// Issue #222: index of the saved item-set the user most recently
    /// activated via the switcher [`egui::ComboBox`]. `None` means "no
    /// switcher selection yet" — typically because the user hasn't
    /// opened the dropdown or because every named set was deleted.
    /// Used purely to drive the closed-combo label and the "active"
    /// marker; the engine's `character.items` is the source of truth
    /// for what's actually equipped.
    pub active_item_set_idx: Option<usize>,
    /// Issue #222: whether the "Manage sets…" popup is open. The popup
    /// hosts the rename / clone / delete actions for each saved set in
    /// one place so the top row stays compact (the inline buttons
    /// remain available for one-click access).
    pub manage_sets_open: bool,
    /// Issue #212 (slice 2): paste-text buffer for the "Import item set"
    /// section of the manage popup. The user pastes a `MK2SET|...` code
    /// produced by [`pob_engine::export_item_set`] (from another build
    /// session); pressing Import decodes it and appends a new
    /// [`pob_engine::character::NamedItemSet`] to `character.item_sets`.
    pub item_set_import_buffer: String,
    /// Issue #212 (slice 2): last error from the import button, surfaced
    /// inline so the popup can explain why decode failed (wrong prefix,
    /// bad base64, malformed JSON). Cleared on successful import or
    /// when the popup closes.
    pub item_set_import_error: Option<String>,
    /// Issue #221 (enchant slice): whether the helmet "Apply
    /// Enchantment" picker popup is open. Surfaces only when the
    /// active slot is `Slot::Helmet` and `app.helmet_enchants` is
    /// loaded — both gates live in the caller.
    pub enchant_picker_open: bool,
    /// Issue #221: live search filter for the helmet enchant picker.
    /// Matched case-insensitively against the skill name.
    pub enchant_picker_filter: String,
    /// Issue #221: which tier the picker commits when the user
    /// clicks a row. Defaults to Endgame (Eternal Lab) — the
    /// stronger roll and what most builds target.
    pub enchant_picker_tier: pob_data::HelmetEnchantTier,
    /// Issue #221 (glove/boot slice): which flat-tier the
    /// glove/boot picker commits. String-typed because the tier
    /// pool differs per slot (NORMAL/CRUEL/MERCILESS for gloves;
    /// CRUEL/MERCILESS for boots); empty means "the first tier in
    /// the catalogue" and the picker auto-fills on first open.
    pub flat_enchant_picker_tier: String,
    /// Issue #221 (anoint slice): popup state for the amulet
    /// anointment picker. Owned here so the search filter survives
    /// across frames while the popup is open.
    pub anoint_picker: crate::anoint_picker::AnointPickerState,
}

/// Issue #211 (slice 3): edit-buffer state for the shared-items rename
/// popup. Pulled into its own struct so the popup body and the
/// context-menu wiring can pass it around without grabbing the whole
/// `ItemsTabState`.
#[derive(Debug, Clone)]
pub struct SharedRenameState {
    /// Index into [`SharedItemStore::items`] when the popup was opened.
    /// Re-validated on Save in case the underlying list shrank.
    pub index: usize,
    /// Live text-edit buffer. Seeded with the current label so the user
    /// can edit-in-place instead of retyping from scratch.
    pub buffer: String,
}

/// Issue #212 (slice 1): edit-buffer state for the item-set rename popup.
/// Mirrors [`SharedRenameState`] but tracks the last validation error so
/// the popup can show it inline (e.g. "Name already in use") instead of
/// silently rejecting the click.
#[derive(Debug, Clone)]
pub struct ItemSetRenameState {
    /// Index into [`Character::item_sets`] when the popup was opened.
    /// Re-validated on Save in case the list shrank in another action.
    pub index: usize,
    /// Live text-edit buffer. Seeded with the existing set name.
    pub buffer: String,
    /// Last error returned by [`rename_item_set`], surfaced inline so
    /// the user understands why Save did nothing. `None` on first open.
    pub last_error: Option<ItemSetOpError>,
}

// Issue #222: the dropdown-label formatter moved to `crate::set_switcher`
// so the future skill-set / config-set switchers can share it. The
// `pub use` keeps the original `items_tab::format_set_dropdown_label`
// path live for any external caller (tests, other tabs).
pub use crate::set_switcher::format_set_dropdown_label;

/// Issue #221: per-slot enchant catalogues bundled together so the
/// items-tab signature doesn't grow one parameter per slot. Each
/// field is `None` when the matching `data/enchants_*.json` file
/// isn't loaded; the picker render gates on the relevant field at
/// the per-slot dispatch site.
#[derive(Debug, Clone, Default)]
pub struct LoadedEnchants {
    /// Skill-keyed catalogue with two named tiers — unique to helmets.
    pub helmet: Option<pob_data::HelmetEnchantSet>,
    /// Flat tier-keyed catalogue for gloves.
    pub gloves: Option<pob_data::FlatEnchantSet>,
    /// Flat tier-keyed catalogue for boots.
    pub boots: Option<pob_data::FlatEnchantSet>,
    /// Flat tier-keyed catalogue for body armour.
    pub body: Option<pob_data::FlatEnchantSet>,
    /// Flat tier-keyed catalogue for belts.
    pub belt: Option<pob_data::FlatEnchantSet>,
    /// Flat tier-keyed catalogue shared across both 1H and 2H weapons.
    pub weapon: Option<pob_data::FlatEnchantSet>,
    /// Flat tier-keyed catalogue for flasks.
    pub flask: Option<pob_data::FlatEnchantSet>,
}

impl LoadedEnchants {
    /// Resolve the flat catalogue for a slot, if any. Returns `None`
    /// for slots that either have no enchant catalogue (Amulet,
    /// Ring1/2) or use the skill-keyed helmet shape (Helmet — the
    /// caller dispatches to `self.helmet` directly for that one).
    #[must_use]
    pub fn flat_for(&self, slot: pob_data::Slot) -> Option<&pob_data::FlatEnchantSet> {
        match slot {
            pob_data::Slot::Gloves => self.gloves.as_ref(),
            pob_data::Slot::Boots => self.boots.as_ref(),
            pob_data::Slot::BodyArmour => self.body.as_ref(),
            pob_data::Slot::Belt => self.belt.as_ref(),
            pob_data::Slot::Weapon1
            | pob_data::Slot::Weapon2
            | pob_data::Slot::Weapon1Swap
            | pob_data::Slot::Weapon2Swap => self.weapon.as_ref(),
            pob_data::Slot::Flask1
            | pob_data::Slot::Flask2
            | pob_data::Slot::Flask3
            | pob_data::Slot::Flask4
            | pob_data::Slot::Flask5 => self.flask.as_ref(),
            _ => None,
        }
    }
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
            shared_rename: None,
            item_set_rename: None,
            top_contributors_open: false,
            top_contributors_axis: crate::node_power_heatmap::HeatmapStat::default(),
            active_item_set_idx: None,
            manage_sets_open: false,
            item_set_import_buffer: String::new(),
            item_set_import_error: None,
            enchant_picker_open: false,
            enchant_picker_filter: String::new(),
            enchant_picker_tier: pob_data::HelmetEnchantTier::default(),
            flat_enchant_picker_tier: String::new(),
            anoint_picker: crate::anoint_picker::AnointPickerState::default(),
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
            mod_lines.push(ModLine::new(trimmed.to_owned(), ModSection::Implicit));
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
        variants: Vec::new(),
        variant: None,
    }
}

/// Outcome of a rename/clone attempt on `character.item_sets`. Distinguishing
/// between "out of range" and "name conflict" lets the UI surface a meaningful
/// error to the user (toast / inline label) instead of just no-op'ing.
///
/// Issue #212 (slice 1, data layer): full CRUD on item sets. `delete_item_set`
/// and `save_item_set` already live on `Character`; rename/clone are pure
/// transformations on `character.item_sets` so we keep them in the UI crate to
/// avoid bloating the engine API.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ItemSetOpError {
    /// `idx` did not point at a real entry in `character.item_sets`.
    OutOfRange,
    /// New name is empty / whitespace-only — meaningless as a label.
    EmptyName,
    /// New name collides with an existing set's name (case-sensitive, matching
    /// `save_item_set`'s position-by-name semantics). The caller can either
    /// pick a different name or, for rename, accept the no-op.
    DuplicateName,
}

/// Rename `character.item_sets[idx]` to `new_name`. Returns
/// `Err(OutOfRange)` if `idx` is past the end, `Err(EmptyName)` if the new
/// name is blank, and `Err(DuplicateName)` if any *other* set already uses
/// that name. Renaming a set to its own name is a no-op success — avoids
/// noisy errors when the user clicks rename, types nothing, and confirms.
pub fn rename_item_set(
    character: &mut Character,
    idx: usize,
    new_name: &str,
) -> Result<(), ItemSetOpError> {
    let trimmed = new_name.trim();
    if trimmed.is_empty() {
        return Err(ItemSetOpError::EmptyName);
    }
    if idx >= character.item_sets.len() {
        return Err(ItemSetOpError::OutOfRange);
    }
    if character.item_sets[idx].name == trimmed {
        return Ok(());
    }
    if character
        .item_sets
        .iter()
        .enumerate()
        .any(|(i, s)| i != idx && s.name == trimmed)
    {
        return Err(ItemSetOpError::DuplicateName);
    }
    character.item_sets[idx].name = trimmed.to_owned();
    Ok(())
}

/// Clone `character.item_sets[idx]` and insert the copy immediately after the
/// source. The new entry gets a unique name derived from the source (suffix
/// " (copy)", " (copy 2)", … until free). Returns the new index, or
/// `Err(OutOfRange)`.
///
/// Implementation note: we go through `Character::save_item_set` to avoid
/// reaching into the engine crate's private `NamedItemSet` constructor —
/// `save_item_set` clones the *currently active* `character.items` under a
/// name. We temporarily swap the source set's items into `character.items`,
/// save under a fresh name (appending to the end), restore the original
/// active items, then rotate the new entry into the desired slot. The active
/// `character.items` is preserved across the call.
pub fn clone_item_set(character: &mut Character, idx: usize) -> Result<usize, ItemSetOpError> {
    if idx >= character.item_sets.len() {
        return Err(ItemSetOpError::OutOfRange);
    }
    let source_items = character.item_sets[idx].items.clone();
    let source_name = character.item_sets[idx].name.clone();
    let new_name = unique_clone_name(&source_name, character);

    // Stash the active items, swap in the source's items, save under the
    // new name (which pushes to the tail), then put the active items back.
    let saved_active = std::mem::replace(&mut character.items, source_items);
    let pushed_idx = character.save_item_set(new_name);
    character.items = saved_active;

    // `save_item_set` appended at the tail; rotate it to live right after
    // the source.
    let target_idx = idx + 1;
    if pushed_idx != target_idx {
        let entry = character.item_sets.remove(pushed_idx);
        character.item_sets.insert(target_idx, entry);
    }
    Ok(target_idx)
}

// Issue #222: the active-idx shift rule moved to `crate::set_switcher`
// so the future skill-set / config-set switchers can share it. The
// `pub use` keeps the original `items_tab::shift_active_idx_after_delete`
// path live for inline-chip / manage-popup callers below.
pub use crate::set_switcher::{shift_active_idx_after_delete, shift_active_idx_after_swap};

/// Issue #222: format the tooltip body for one saved set in the switcher
/// dropdown.
///
/// PoB's set dropdowns surface a small per-row summary on hover — slot
/// count plus mod-line count — so the user can size up a set without
/// committing to the load. We replicate that here as a pure helper so the
/// tooltip body stays unit-testable. Returned as a `Vec<String>` so the
/// caller can either `.join("\n")` it into `egui::Ui::on_hover_text` or
/// render the lines individually inside a custom hover layout.
///
/// Empty / whitespace-only names fall back to `(unnamed)` — same rule as
/// [`crate::set_switcher::format_set_dropdown_label`] so the closed combo
/// and the hover stay consistent.
#[must_use]
pub fn format_item_set_tooltip_lines(set: &NamedItemSet) -> Vec<String> {
    let trimmed = set.name.trim();
    let display = if trimmed.is_empty() {
        "(unnamed)"
    } else {
        trimmed
    };
    let slots_filled = set.items.items.len();
    let mod_lines: usize = set
        .items
        .items
        .values()
        .map(|item| item.mod_lines.len())
        .sum();
    vec![
        display.to_owned(),
        format!("Slots filled: {slots_filled}"),
        format!("Mod lines: {mod_lines}"),
    ]
}

/// Issue #212 follow-up: direction the user picked on a reorder
/// arrow-button click. Up moves the entry toward index 0; Down moves
/// toward the tail. The helper [`move_item_set`] consumes this enum
/// so the call site doesn't have to do its own bounds math.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MoveDirection {
    Up,
    Down,
}

/// Issue #212 follow-up: interim non-drag reorder for `character.item_sets`.
/// Swap `idx` with its neighbour in the chosen direction. Returns the
/// post-swap index of the moved entry, or `None` if the move can't
/// happen (out-of-range index, or already at the edge of the list).
///
/// Pure / no I/O — pairs with [`shift_active_idx_after_swap`] in
/// `set_switcher` so the UI's active marker can follow the moved entry
/// without re-reading the list.
pub fn move_item_set(
    character: &mut Character,
    idx: usize,
    direction: MoveDirection,
) -> Option<usize> {
    let len = character.item_sets.len();
    if idx >= len {
        return None;
    }
    let other = match direction {
        MoveDirection::Up => {
            if idx == 0 {
                return None;
            }
            idx - 1
        }
        MoveDirection::Down => {
            if idx + 1 >= len {
                return None;
            }
            idx + 1
        }
    };
    character.item_sets.swap(idx, other);
    Some(other)
}

/// Pick the first free " (copy)" / " (copy 2)" / … suffix for `base`.
fn unique_clone_name(base: &str, character: &Character) -> String {
    let names: std::collections::HashSet<&str> = character
        .item_sets
        .iter()
        .map(|s| s.name.as_str())
        .collect();
    let first = format!("{base} (copy)");
    if !names.contains(first.as_str()) {
        return first;
    }
    for n in 2u32.. {
        let candidate = format!("{base} (copy {n})");
        if !names.contains(candidate.as_str()) {
            return candidate;
        }
    }
    // 2^32 collisions on a single name is effectively impossible, but the
    // loop above is `2u32..` so we need an unreachable terminator for the
    // type-checker.
    unreachable!("ran out of u32 suffixes for {base}")
}

/// Returns true if the equipped items changed (so the caller can recompute).
pub fn ui(
    ui: &mut egui::Ui,
    state: &mut ItemsTabState,
    character: &mut Character,
    tree: &PassiveTree,
    skills: &SkillRegistry,
    bases: Option<&ItemBaseSet>,
    shared_items: &mut SharedItemStore,
    enchants: &LoadedEnchants,
) -> bool {
    let mut changed = false;
    let helmet_enchants = enchants.helmet.as_ref();
    // Issue #222: item-set switcher + manage popup. Above the existing
    // per-set chip row we render a single `ComboBox` listing every saved
    // set — picking one calls `Character::activate_item_set` (mirroring
    // the inline "Load" button). A "Manage sets…" button next to it
    // opens a popup that surfaces rename / clone / delete in one place
    // so the chip row stays compact on builds with many sets.
    ui.horizontal_wrapped(|ui| {
        ui.label("Active set:");
        let total = character.item_sets.len();
        // Clamp the remembered index so a delete-while-popup-closed
        // doesn't leave a dangling out-of-range pointer.
        if let Some(idx) = state.active_item_set_idx {
            if idx >= total {
                state.active_item_set_idx = None;
            }
        }
        let selected_idx = state.active_item_set_idx;
        let closed_label = match selected_idx.and_then(|i| character.item_sets.get(i)) {
            Some(set) => format_set_dropdown_label(&set.name, selected_idx.unwrap(), true),
            None if total == 0 => "(no saved sets)".to_owned(),
            None => "Pick a saved set…".to_owned(),
        };
        // Snapshot the names so the combo body doesn't hold an
        // immutable borrow of `character.item_sets` while it tries to
        // call `character.activate_item_set` (which needs `&mut self`).
        let entries: Vec<(usize, String)> = character
            .item_sets
            .iter()
            .enumerate()
            .map(|(i, s)| (i, s.name.clone()))
            .collect();
        let combo = egui::ComboBox::from_id_salt("item-set-switcher")
            .width(220.0)
            .selected_text(closed_label);
        combo.show_ui(ui, |ui| {
            if entries.is_empty() {
                ui.weak("Save a set below to populate the switcher.");
                return;
            }
            for (idx, name) in &entries {
                let is_active = selected_idx == Some(*idx);
                let label = format_set_dropdown_label(name, *idx, is_active);
                // Issue #222: per-row hover surfaces slot + mod-line
                // counts so the user can size up a set without
                // committing to the load.
                let tooltip = character
                    .item_sets
                    .get(*idx)
                    .map(|set| format_item_set_tooltip_lines(set).join("\n"))
                    .unwrap_or_default();
                let resp = ui.selectable_label(is_active, label).on_hover_text(tooltip);
                if resp.clicked() && !is_active {
                    if character.activate_item_set(*idx) {
                        state.active_item_set_idx = Some(*idx);
                        changed = true;
                    }
                }
            }
        });
        if ui
            .add_enabled(total > 0, egui::Button::new("Manage sets…"))
            .on_hover_text("Open the rename / clone / delete popup for saved item sets.")
            .clicked()
        {
            state.manage_sets_open = true;
        }
    });
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
                        // Issue #222: keep the switcher dropdown in sync
                        // with the inline chip's load action so both
                        // entry points agree on the active set.
                        state.active_item_set_idx = Some(idx);
                        changed = true;
                    }
                }
                // Issue #212 (slice 1): rename — opens the popup
                // seeded with the current name. Validation lives in
                // `rename_item_set`; the popup surfaces errors inline.
                if ui
                    .small_button("✎")
                    .on_hover_text(format!("Rename {name}"))
                    .clicked()
                {
                    state.item_set_rename = Some(ItemSetRenameState {
                        index: idx,
                        buffer: name.clone(),
                        last_error: None,
                    });
                }
                // Issue #212 (slice 1): clone — duplicates the set in
                // place (inserted right after the source). Auto-named
                // " (copy)" / " (copy 2)" / … to avoid collisions.
                if ui
                    .small_button("⎘")
                    .on_hover_text(format!("Clone {name}"))
                    .clicked()
                {
                    let _ = clone_item_set(character, idx);
                    // No recompute — cloning a saved (inactive) set
                    // doesn't change `character.items`.
                }
                if ui
                    .small_button("✕")
                    .on_hover_text(format!("Delete {name}"))
                    .clicked()
                {
                    if character.delete_item_set(idx) {
                        // Issue #222: keep the switcher pointer
                        // consistent (clear on self-delete, shift down
                        // on earlier-delete). No recompute — deleting a
                        // saved (inactive) set doesn't change
                        // `character.items`.
                        state.active_item_set_idx =
                            shift_active_idx_after_delete(state.active_item_set_idx, idx);
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
    // Issue #212 (slice 1): rename popup. Drawn at the top of the tab so
    // it floats over whatever the user is doing.
    render_item_set_rename_popup(ui, character, &mut state.item_set_rename);
    // Issue #222: manage-sets popup. Same actions as the inline chip
    // row, surfaced in a dedicated window so the top bar stays compact
    // on builds with many saved sets. Returns true if a load happened
    // inside the popup so the calc engine recomputes.
    if render_manage_sets_popup(ui, character, state) {
        changed = true;
    }
    // Issue #221 (picker slice): the Apply-Enchantment popup.
    // Dispatch on the active slot — helmets use the skill-keyed
    // `HelmetEnchantSet` picker; every other enchant slot uses the
    // flat-tier picker via `LoadedEnchants::flat_for`. Returns
    // true when a pick committed so the calc engine recomputes.
    let picker_changed = match state.selected_slot {
        Some(Slot::Helmet) | None => {
            render_enchant_picker_popup(ui, character, state, helmet_enchants)
        }
        Some(slot) => {
            render_flat_enchant_picker_popup(ui, character, state, slot, enchants.flat_for(slot))
        }
    };
    if picker_changed {
        changed = true;
    }
    // Issue #221 (anoint slice): the Apply-Anointment popup. Same
    // commit shape as the lab-enchant pickers (lands on
    // `ModSection::Enchant`); the catalogue is the live passive
    // tree's notable nodes, surfaced via `anoint_picker`.
    if crate::anoint_picker::render_picker_popup(ui, character, &mut state.anoint_picker, tree) {
        changed = true;
    }
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
                            // Issue #225 (color-code coverage): item names
                            // and mod lines from PoB carry `^N` / `^xRRGGBB`
                            // escapes (uniques use them for the gold
                            // namebar, corrupted lines for red text, etc.).
                            // Route through `label_with_escapes` so the
                            // tooltip shows them coloured instead of as
                            // raw escape characters.
                            for line in &lines {
                                color_codes::label_with_escapes(ui, line);
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
                    let mut socket_click = SocketClick::default();
                    egui::ScrollArea::vertical()
                        .max_height(220.0)
                        .show(ui, |ui| {
                            socket_click = render_item_summary(ui, &item);
                        });
                    // Issue #221 follow-up: a dot click cycles the
                    // socket's colour; a gap click toggles the link
                    // separator. Pure helpers do the string work; we
                    // just plumb the new string back into the live
                    // ItemSet and dirty-flag the build so the auto-save
                    // / Save button picks it up.
                    if let Some(idx) = socket_click.dot {
                        if let Some(item_mut) = items.get_mut(slot) {
                            let new_sockets = crate::socket_renderer::apply_socket_cycle_at(
                                &item_mut.sockets,
                                idx,
                            );
                            if new_sockets != item_mut.sockets {
                                item_mut.sockets = new_sockets;
                                changed = true;
                            }
                        }
                    } else if let Some(idx) = socket_click.link {
                        if let Some(item_mut) = items.get_mut(slot) {
                            let new_sockets = crate::socket_renderer::apply_socket_link_toggle_at(
                                &item_mut.sockets,
                                idx,
                            );
                            if new_sockets != item_mut.sockets {
                                item_mut.sockets = new_sockets;
                                changed = true;
                            }
                        }
                    }
                    ui.add_space(4.0);
                    // Issue #221: variant picker. Only renders for
                    // items that declared `Variant:` entries (the
                    // common case is empty, so most slots see no
                    // dropdown). Picking a different variant flips
                    // `item.variant`, rewrites the `Selected Variant:`
                    // line in `item.raw` (so PoB-XML export round-trips
                    // the new pick), and triggers a recompute so the
                    // Calcs tab reflects the gated mods.
                    if !item.variants.is_empty() {
                        let mut current = item.variant.unwrap_or(1);
                        let before = current;
                        ui.horizontal(|ui| {
                            ui.label("Variant:");
                            let active_label = item
                                .variants
                                .get(current.saturating_sub(1) as usize)
                                .cloned()
                                .unwrap_or_else(|| current.to_string());
                            egui::ComboBox::from_id_salt(("variant-combo", slot.label()))
                                .selected_text(active_label)
                                .show_ui(ui, |ui| {
                                    for (idx, name) in item.variants.iter().enumerate() {
                                        let id = u32::try_from(idx + 1).unwrap_or(1);
                                        ui.selectable_value(
                                            &mut current,
                                            id,
                                            format!("{id}. {name}"),
                                        );
                                    }
                                });
                        });
                        if current != before {
                            if let Some(equipped_mut) = items.get_mut(slot) {
                                equipped_mut.set_active_variant(current);
                                changed = true;
                            }
                        }
                    }
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
                        // Issue #221 (picker slice): "Apply
                        // Enchantment" renders on every slot that
                        // has a matching catalogue loaded. Helmet
                        // uses the skill-keyed `HelmetEnchantSet`
                        // shape; every other slot (gloves, boots,
                        // body, belt, weapons, flasks) uses the
                        // flat `FlatEnchantSet` shape via
                        // `LoadedEnchants::flat_for`.
                        let has_catalogue = if slot == Slot::Helmet {
                            helmet_enchants.is_some()
                        } else {
                            enchants.flat_for(slot).is_some()
                        };
                        if has_catalogue
                            && ui
                                .button("Apply Enchantment…")
                                .on_hover_text(
                                    "Pick an enchant from the lab catalogue. \
                                     Replaces any existing enchant on this slot.",
                                )
                                .clicked()
                        {
                            state.enchant_picker_open = true;
                            state.enchant_picker_filter.clear();
                        }
                        // Issue #221 (anoint slice): "Apply
                        // Anointment…" lives on the Amulet slot.
                        // Anoint mods are notable stats from the
                        // live passive tree (no separate catalogue
                        // — the tree itself is the data source), so
                        // the picker dispatches into
                        // `anoint_picker` instead of the lab-enchant
                        // path.
                        if slot == Slot::Amulet
                            && ui
                                .button("Apply Anointment…")
                                .on_hover_text(
                                    "Pick a passive-tree notable to anoint onto \
                                     this amulet. Replaces any existing anointment.",
                                )
                                .clicked()
                        {
                            state.anoint_picker.open = true;
                            state.anoint_picker.filter.clear();
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
                            &mut state.shared_rename,
                            use_swap,
                        ) {
                            changed = true;
                        }
                    }
                }
            });
        }
    });

    // Issue #207 (panel slice): "Top contributing mod lines" power
    // report. Wired into PR #379's `format_top_contributors` formatter
    // — opt-in collapsing header so the M+1-perform-call-per-equipped-
    // item cost only burns when the user actually wants it. Read-only,
    // never sets `changed`.
    ui.separator();
    egui::CollapsingHeader::new("Top contributing mod lines")
        .id_salt("items_tab_top_contributors")
        .default_open(false)
        .show(ui, |ui| {
            state.top_contributors_open = true;
            ui.label(
                "Removes one mod line at a time and re-runs the build to score what \
                 each line is actually worth. Slow on full equipped sets.",
            );
            // Issue #207 follow-up: per-axis sort selector. Mirrors the
            // heatmap stat-axis dropdown so a defence-focused build can
            // foreground its EHP-positive lines (otherwise drowned out
            // by the higher-magnitude DPS column).
            ui.horizontal(|ui| {
                ui.label("Sort by:");
                use crate::node_power_heatmap::HeatmapStat;
                for stat in [HeatmapStat::Combined, HeatmapStat::Dps, HeatmapStat::Ehp] {
                    ui.selectable_value(&mut state.top_contributors_axis, stat, stat.label());
                }
            });
            // Issue #207 follow-up: share the expensive per-slot
            // `rank_item_modlines` walk across both sub-panels (top
            // mod lines + per-slot aggregate). Running them off two
            // separate collections would double the M+1 perform calls.
            let scores = collect_item_modline_scores(character, tree, skills, bases);
            let lines =
                sort_and_format_contributors(scores.clone(), state.top_contributors_axis, 10);
            if lines.is_empty() {
                ui.weak("(no equipped mod lines to score)");
            } else {
                // Issue #225 (color-code coverage): mod lines that bleed
                // into the contributors panel may carry inline PoB
                // escapes — render them properly rather than as raw
                // characters.
                for line in &lines {
                    color_codes::label_with_escapes(ui, line);
                }
            }
            // Issue #207 follow-up: slot-aggregate view. Shows which
            // slot is doing the most work in aggregate — useful when
            // a single big-impact mod hides a more diffuse pattern
            // (e.g. five small +DPS rolls on the same chest beating
            // one big roll on a glove).
            ui.add_space(6.0);
            egui::CollapsingHeader::new("Top contributing slots")
                .id_salt("items_tab_top_slots")
                .default_open(false)
                .show(ui, |ui| {
                    let by_slot =
                        aggregate_contributors_by_slot(&scores, state.top_contributors_axis);
                    if by_slot.is_empty() {
                        ui.weak("(no equipped slots to aggregate)");
                    } else {
                        for line in format_slot_contributions(&by_slot) {
                            ui.monospace(line);
                        }
                    }
                });
        });

    changed
}

/// Issue #207 (panel slice): build the top-N contributing mod-line
/// strings across every equipped slot. Calls
/// [`rank_item_modlines`](pob_engine::rank_item_modlines) per slot, then
/// re-sorts the flattened result by max(dps_delta, ehp_delta) descending
/// before handing off to [`format_top_contributors`]. Returns an empty
/// vector when nothing is equipped — the panel renders an empty-state
/// message in that case.
///
/// Cluster context and timeless data are passed as `None` here: the
/// panel is intentionally cheap-to-wire and approximate. Callers that
/// need cluster-aware scoring will add the threading in a follow-up
/// alongside the heatmap work.
/// Issue #207 follow-up: shared collection step for the top-contributors
/// panel and the new slot-aggregate view. Runs `rank_item_modlines`
/// once per equipped slot and concatenates the results. Callers that
/// want both views should call this once and feed the same `Vec` into
/// [`sort_and_format_contributors`] and
/// [`aggregate_contributors_by_slot`] — running the collection twice
/// pays the M+1-perform-calls cost twice.
pub fn collect_item_modline_scores(
    character: &Character,
    tree: &PassiveTree,
    skills: &SkillRegistry,
    bases: Option<&ItemBaseSet>,
) -> Vec<ItemModlineScore> {
    let mut all: Vec<ItemModlineScore> = Vec::new();
    for slot in Slot::all() {
        if character.items.get(*slot).is_none() {
            continue;
        }
        let scores = rank_item_modlines(character, tree, *slot, Some(skills), bases, None, None);
        all.extend(scores);
    }
    all
}

/// Issue #207 follow-up: per-slot aggregate of `(dps_delta, ehp_delta)`
/// over every mod line on that slot, returned sorted descending by the
/// chosen [`HeatmapStat`](crate::node_power_heatmap::HeatmapStat) axis.
/// Pure helper so the slot ranking is testable without the expensive
/// `rank_item_modlines` walk.
///
/// Slots with no equipped item simply don't appear (the input never
/// lists them); slots whose total is zero on the chosen axis stay in
/// the result but sink to the bottom.
#[must_use]
pub fn aggregate_contributors_by_slot(
    scores: &[ItemModlineScore],
    axis: crate::node_power_heatmap::HeatmapStat,
) -> Vec<(pob_data::Slot, f64, f64)> {
    use crate::node_power_heatmap::HeatmapStat;
    use ahash::AHashMap;
    let mut totals: AHashMap<pob_data::Slot, (f64, f64)> = AHashMap::new();
    for s in scores {
        let entry = totals.entry(s.slot).or_insert((0.0, 0.0));
        entry.0 += s.dps_delta;
        entry.1 += s.ehp_delta;
    }
    let mut out: Vec<(pob_data::Slot, f64, f64)> = totals
        .into_iter()
        .map(|(slot, (dps, ehp))| (slot, dps, ehp))
        .collect();
    let key = |entry: &(pob_data::Slot, f64, f64)| -> f64 {
        match axis {
            HeatmapStat::Combined => entry.1.max(entry.2),
            HeatmapStat::Dps => entry.1,
            HeatmapStat::Ehp => entry.2,
        }
    };
    out.sort_by(|a, b| {
        key(b)
            .partial_cmp(&key(a))
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    out
}

/// Issue #207 follow-up: format the slot-aggregate output as
/// human-readable rows for the "Top slots" sub-panel. Each row carries
/// the slot label, signed DPS / EHP totals.
#[must_use]
pub fn format_slot_contributions(aggregated: &[(pob_data::Slot, f64, f64)]) -> Vec<String> {
    aggregated
        .iter()
        .map(|(slot, dps, ehp)| format!("{:>+8.0} DPS {:>+8.0} EHP  {}", dps, ehp, slot.label()))
        .collect()
}

/// Issue #207 follow-up: sort + format the equipped-set contributors
/// by the chosen [`crate::node_power_heatmap::HeatmapStat`] axis. Pure
/// helper so the axis-specific sort behaviour is unit-testable without
/// reaching for `rank_item_modlines` and a full character fixture.
#[must_use]
pub fn sort_and_format_contributors(
    mut scores: Vec<ItemModlineScore>,
    axis: crate::node_power_heatmap::HeatmapStat,
    top_n: usize,
) -> Vec<String> {
    use crate::node_power_heatmap::HeatmapStat;
    let key = |s: &ItemModlineScore| -> f64 {
        match axis {
            HeatmapStat::Combined => s.dps_delta.max(s.ehp_delta),
            HeatmapStat::Dps => s.dps_delta,
            HeatmapStat::Ehp => s.ehp_delta,
        }
    };
    scores.sort_by(|a, b| {
        key(b)
            .partial_cmp(&key(a))
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    format_top_contributors(&scores, top_n)
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
    rename_state: &mut Option<SharedRenameState>,
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
    let mut to_clone: Option<usize> = None;
    let mut to_rename: Option<usize> = None;
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
                    lines.push("Right-click for more actions.".into());
                    let row = ui
                        .add(egui::Label::new(label_text).sense(egui::Sense::click()))
                        .on_hover_ui(|ui| {
                            // Issue #225 (color-code coverage): shared-item
                            // tooltip mirrors the equipped-slot tooltip; both
                            // route through `label_with_escapes` so unique
                            // names + escape-bearing mod lines render with
                            // their PoB colours.
                            for line in &lines {
                                if line.is_empty() {
                                    ui.add_space(4.0);
                                } else {
                                    color_codes::label_with_escapes(ui, line);
                                }
                            }
                        });
                    if row.double_clicked() {
                        let dest = target.or(*selected_slot);
                        if let Some(slot) = dest {
                            to_equip = Some((slot, saved.item.clone()));
                        }
                    }
                    // Issue #211 (slice 2): right-click context menu —
                    // mirrors PoB's `SharedItemListControl` row menu
                    // (clone / delete). Equip stays on double-click so
                    // the menu doesn't need a redundant entry; the
                    // hover-tooltip's bottom line teaches the gesture.
                    row.context_menu(|ui| {
                        if ui.button("Rename").clicked() {
                            to_rename = Some(idx);
                            ui.close_menu();
                        }
                        if ui.button("Clone").clicked() {
                            to_clone = Some(idx);
                            ui.close_menu();
                        }
                        if ui.button("Delete").clicked() {
                            to_delete = Some(idx);
                            ui.close_menu();
                        }
                    });
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
    if let Some(idx) = to_clone {
        // `clone_at` already runs `unique_label` so the new row gets a
        // `(2)` / `(3)` suffix and `dirty` flips — the per-frame flush
        // in lib.rs picks it up. No `changed` flip because the saved
        // list isn't part of the perform pipeline.
        shared_items.clone_at(idx);
    }
    if let Some(idx) = to_delete {
        shared_items.remove(idx);
        // Removal mutates the store; the caller's per-frame flush picks
        // it up via `dirty`. We don't return `changed = true` here
        // because no calc-affecting state changed — just the saved-list.
    }
    // Issue #211 (slice 3): opening the rename popup is just state — we
    // seed the buffer with the current label so the user edits in place
    // instead of retyping from scratch. The popup itself renders below.
    if let Some(idx) = to_rename {
        if let Some(saved) = shared_items.items.get(idx) {
            *rename_state = Some(SharedRenameState {
                index: idx,
                buffer: saved.label.clone(),
            });
        }
    }
    render_shared_rename_popup(ui, shared_items, rename_state);
    changed
}

/// Issue #211 (slice 3): inline rename popup for a shared-items row.
/// Renders an `egui::Window` with a `TextEdit` + Save / Cancel buttons.
/// Save commits via [`SharedItemStore::rename`] which routes through
/// `unique_label` on collisions and rejects empty / whitespace-only
/// labels (same contract as `add`). Cancel-on-Esc and close-on-outside-
/// click come for free from `egui::Window::open` — we drive both
/// through the same `*rename_state = None` reset.
fn render_shared_rename_popup(
    ui: &mut egui::Ui,
    shared_items: &mut SharedItemStore,
    rename_state: &mut Option<SharedRenameState>,
) {
    let Some(state) = rename_state.as_mut() else {
        return;
    };
    // Pull the current label up-front so the title reflects the row the
    // user right-clicked, even if the buffer has diverged.
    let current_label = shared_items
        .items
        .get(state.index)
        .map(|s| s.label.clone())
        .unwrap_or_else(|| "(removed)".to_owned());

    let mut window_open = true;
    let mut commit = false;
    let mut cancel = false;
    egui::Window::new(format!("Rename \"{current_label}\""))
        .id(egui::Id::new(("shared-rename-window", state.index)))
        .open(&mut window_open)
        .resizable(false)
        .collapsible(false)
        .default_width(280.0)
        .show(ui.ctx(), |ui| {
            ui.label("New label:");
            let response = ui.add(
                egui::TextEdit::singleline(&mut state.buffer)
                    .desired_width(f32::INFINITY)
                    .hint_text("e.g. Endgame boots"),
            );
            // Keep focus on the TextEdit so the user can immediately
            // type — without this the popup steals the click but not
            // keyboard focus on first frame.
            if !response.has_focus() && response.gained_focus() {
                response.request_focus();
            }
            response.request_focus();
            let empty = state.buffer.trim().is_empty();
            if empty {
                ui.weak("Label can't be blank.");
            }
            ui.horizontal(|ui| {
                let save_clicked = ui.add_enabled(!empty, egui::Button::new("Save")).clicked();
                let enter_pressed = response.lost_focus()
                    && ui.input(|i| i.key_pressed(egui::Key::Enter))
                    && !empty;
                if save_clicked || enter_pressed {
                    commit = true;
                }
                if ui.button("Cancel").clicked() {
                    cancel = true;
                }
            });
            // Esc cancels — matches the "outside-click closes" gesture
            // users expect from a transient popup.
            if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                cancel = true;
            }
        });
    if commit {
        // `rename` rejects empty / whitespace and routes collisions
        // through `unique_label`. We don't bail on `false` — at this
        // point the only failure mode is an out-of-range index (the
        // row was deleted in another action this frame), in which case
        // closing the popup is the right outcome anyway.
        let new_label = std::mem::take(&mut state.buffer);
        shared_items.rename(state.index, &new_label);
        *rename_state = None;
    } else if cancel || !window_open {
        *rename_state = None;
    }
}

/// Issue #221 (glove/boot picker slice): render the "Apply
/// Enchantment" popup for slots whose catalogue lives in the flat
/// `{tier: [mods]}` shape (gloves, boots). The user picks a tier
/// (radio buttons over whatever tiers the catalogue lists) and one
/// mod line from the pool; commit applies it via
/// [`pob_data::Item::apply_enchant`] like the helmet picker.
///
/// Returns `true` when a pick committed so the caller dirty-flags
/// + recomputes.
fn render_flat_enchant_picker_popup(
    ui: &mut egui::Ui,
    character: &mut Character,
    state: &mut ItemsTabState,
    slot: Slot,
    catalogue: Option<&pob_data::FlatEnchantSet>,
) -> bool {
    if !state.enchant_picker_open {
        return false;
    }
    let Some(catalogue) = catalogue else {
        state.enchant_picker_open = false;
        return false;
    };
    if catalogue.is_empty() {
        state.enchant_picker_open = false;
        return false;
    }
    // Seed the tier on first open. The user can flip between tiers
    // via the radio row below; this only fires when the saved
    // selection is missing from the catalogue (e.g. boot tier
    // remembered from a previous glove popup).
    let tier_keys: Vec<String> = catalogue.iter().map(|(k, _)| k.clone()).collect();
    if !tier_keys
        .iter()
        .any(|k| k == &state.flat_enchant_picker_tier)
    {
        state.flat_enchant_picker_tier = tier_keys.first().cloned().unwrap_or_default();
    }
    let mut window_open = true;
    let mut committed = false;
    let mut chosen_pick: Option<String> = None;
    let mut clear_enchant = false;
    let title = format!("Apply {} Enchantment", slot.label());
    egui::Window::new(title)
        .id(egui::Id::new(("flat-enchant-picker", slot.label())))
        .open(&mut window_open)
        .resizable(true)
        .collapsible(false)
        .default_width(460.0)
        .default_height(440.0)
        .show(ui.ctx(), |ui| {
            ui.horizontal_wrapped(|ui| {
                ui.label("Tier:");
                for key in &tier_keys {
                    ui.selectable_value(
                        &mut state.flat_enchant_picker_tier,
                        key.clone(),
                        key.as_str(),
                    );
                }
            });
            ui.horizontal(|ui| {
                ui.label("Search:");
                ui.add(
                    egui::TextEdit::singleline(&mut state.enchant_picker_filter)
                        .desired_width(260.0)
                        .hint_text("mod text"),
                );
                if ui.small_button("✕").on_hover_text("Clear search").clicked() {
                    state.enchant_picker_filter.clear();
                }
            });
            ui.separator();
            let filter_lc = state.enchant_picker_filter.to_ascii_lowercase();
            let lines = catalogue.lines_for(&state.flat_enchant_picker_tier);
            egui::ScrollArea::vertical()
                .id_salt("flat-enchant-list")
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    for line in lines {
                        if !filter_lc.is_empty() && !line.to_ascii_lowercase().contains(&filter_lc)
                        {
                            continue;
                        }
                        let resp =
                            ui.add(egui::Label::new(line).wrap().sense(egui::Sense::click()));
                        if resp.clicked() {
                            chosen_pick = Some(line.clone());
                        }
                        ui.separator();
                    }
                });
            ui.horizontal(|ui| {
                if ui
                    .button("Remove enchant")
                    .on_hover_text("Strip any existing enchant from this slot.")
                    .clicked()
                {
                    clear_enchant = true;
                }
                if ui.button("Cancel").clicked() {
                    state.enchant_picker_open = false;
                }
            });
        });
    if let Some(line) = chosen_pick {
        if let Some(item) = character.items.get_mut(slot) {
            item.apply_enchant(&[line]);
            committed = true;
        }
        state.enchant_picker_open = false;
    } else if clear_enchant {
        if let Some(item) = character.items.get_mut(slot) {
            item.apply_enchant(&[]);
            committed = true;
        }
        state.enchant_picker_open = false;
    } else if !window_open {
        state.enchant_picker_open = false;
    }
    committed
}

/// Issue #221 (picker slice): render the "Apply Enchantment" popup.
/// Lists every helmet enchant in the loaded catalogue, filtered by a
/// case-insensitive search box; clicking a row applies the selected
/// tier's mod lines to the equipped helmet via
/// [`pob_data::Item::apply_enchant`].
///
/// Returns `true` when a pick committed so the caller can dirty-flag
/// the build + recompute. Returns `false` for clicks that don't
/// commit (cancel, outside-click, no helmet equipped, etc.).
fn render_enchant_picker_popup(
    ui: &mut egui::Ui,
    character: &mut Character,
    state: &mut ItemsTabState,
    catalogue: Option<&pob_data::HelmetEnchantSet>,
) -> bool {
    if !state.enchant_picker_open {
        return false;
    }
    let Some(catalogue) = catalogue else {
        // Defensive — caller already gates the button on this being
        // `Some`, but a race (data file removed mid-session) would
        // otherwise leave the popup stuck open.
        state.enchant_picker_open = false;
        return false;
    };
    let mut window_open = true;
    let mut committed = false;
    let mut chosen_pick: Option<Vec<String>> = None;
    let mut clear_enchant = false;
    egui::Window::new("Apply Helmet Enchantment")
        .id(egui::Id::new("helmet-enchant-picker"))
        .open(&mut window_open)
        .resizable(true)
        .collapsible(false)
        .default_width(420.0)
        .default_height(420.0)
        .show(ui.ctx(), |ui| {
            ui.horizontal(|ui| {
                ui.label("Tier:");
                ui.selectable_value(
                    &mut state.enchant_picker_tier,
                    pob_data::HelmetEnchantTier::Merciless,
                    pob_data::HelmetEnchantTier::Merciless.label(),
                );
                ui.selectable_value(
                    &mut state.enchant_picker_tier,
                    pob_data::HelmetEnchantTier::Endgame,
                    pob_data::HelmetEnchantTier::Endgame.label(),
                );
            });
            ui.horizontal(|ui| {
                ui.label("Search:");
                ui.add(
                    egui::TextEdit::singleline(&mut state.enchant_picker_filter)
                        .desired_width(220.0)
                        .hint_text("skill name"),
                );
                if ui.small_button("✕").on_hover_text("Clear search").clicked() {
                    state.enchant_picker_filter.clear();
                }
            });
            ui.separator();
            let filter_lc = state.enchant_picker_filter.to_ascii_lowercase();
            let tier = state.enchant_picker_tier;
            egui::ScrollArea::vertical()
                .id_salt("enchant-list")
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    for (skill, enchant) in catalogue.iter() {
                        if !filter_lc.is_empty() && !skill.to_ascii_lowercase().contains(&filter_lc)
                        {
                            continue;
                        }
                        let lines = tier.lines(enchant);
                        if lines.is_empty() {
                            continue;
                        }
                        ui.group(|ui| {
                            let header_resp = ui.add(
                                egui::Label::new(egui::RichText::new(skill).strong())
                                    .sense(egui::Sense::click()),
                            );
                            for line in lines {
                                ui.label(format!("  • {line}"));
                            }
                            if header_resp.clicked() || ui.button("Apply").clicked() {
                                chosen_pick = Some(lines.to_vec());
                            }
                        });
                    }
                });
            ui.separator();
            ui.horizontal(|ui| {
                if ui
                    .button("Remove enchant")
                    .on_hover_text("Strip any existing helmet enchant from this slot.")
                    .clicked()
                {
                    clear_enchant = true;
                }
                if ui.button("Cancel").clicked() {
                    state.enchant_picker_open = false;
                }
            });
        });
    if let Some(lines) = chosen_pick {
        if let Some(item) = character.items.get_mut(Slot::Helmet) {
            item.apply_enchant(&lines);
            committed = true;
        }
        state.enchant_picker_open = false;
    } else if clear_enchant {
        if let Some(item) = character.items.get_mut(Slot::Helmet) {
            item.apply_enchant(&[]);
            committed = true;
        }
        state.enchant_picker_open = false;
    } else if !window_open {
        state.enchant_picker_open = false;
    }
    committed
}

/// Issue #212 (slice 1): render the item-set rename popup. Mirrors
/// `render_shared_rename_popup` but drives [`rename_item_set`] and
/// surfaces its `ItemSetOpError` inline so the user sees *why* a
/// duplicate / empty name was rejected. Only commits & closes on
/// success — keeps the popup open with an error message otherwise.
fn render_item_set_rename_popup(
    ui: &mut egui::Ui,
    character: &mut Character,
    rename_state: &mut Option<ItemSetRenameState>,
) {
    let Some(state) = rename_state.as_mut() else {
        return;
    };
    let current_name = character
        .item_sets
        .get(state.index)
        .map(|s| s.name.clone())
        .unwrap_or_else(|| "(removed)".to_owned());

    let mut window_open = true;
    let mut commit = false;
    let mut cancel = false;
    egui::Window::new(format!("Rename \"{current_name}\""))
        .id(egui::Id::new(("item-set-rename-window", state.index)))
        .open(&mut window_open)
        .resizable(false)
        .collapsible(false)
        .default_width(280.0)
        .show(ui.ctx(), |ui| {
            ui.label("New name:");
            let response = ui.add(
                egui::TextEdit::singleline(&mut state.buffer)
                    .desired_width(f32::INFINITY)
                    .hint_text("e.g. Tank build"),
            );
            if !response.has_focus() && response.gained_focus() {
                response.request_focus();
            }
            response.request_focus();
            // Surface the most recent rejection inline so the user
            // doesn't have to guess why Save is bouncing.
            if let Some(err) = state.last_error {
                let msg = match err {
                    ItemSetOpError::EmptyName => "Name can't be blank.",
                    ItemSetOpError::DuplicateName => "Name already in use.",
                    ItemSetOpError::OutOfRange => "Set was removed.",
                };
                ui.colored_label(egui::Color32::LIGHT_RED, msg);
            }
            let empty = state.buffer.trim().is_empty();
            ui.horizontal(|ui| {
                let save_clicked = ui.add_enabled(!empty, egui::Button::new("Save")).clicked();
                let enter_pressed = response.lost_focus()
                    && ui.input(|i| i.key_pressed(egui::Key::Enter))
                    && !empty;
                if save_clicked || enter_pressed {
                    commit = true;
                }
                if ui.button("Cancel").clicked() {
                    cancel = true;
                }
            });
            if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                cancel = true;
            }
        });
    if commit {
        let new_name = state.buffer.clone();
        match rename_item_set(character, state.index, &new_name) {
            Ok(()) => *rename_state = None,
            Err(err) => state.last_error = Some(err),
        }
    } else if cancel || !window_open {
        *rename_state = None;
    }
}

/// Issue #222: render the "Manage sets…" popup window. Lists every saved
/// item set with the same Load / Rename / Clone / Delete actions exposed by
/// the inline chip row, just consolidated in one window so the chip row
/// doesn't grow unboundedly on builds with many sets.
///
/// Returns true if the user activated a set from inside the popup (caller
/// kicks off a recompute the same way the chip row does).
fn render_manage_sets_popup(
    ui: &mut egui::Ui,
    character: &mut Character,
    state: &mut ItemsTabState,
) -> bool {
    if !state.manage_sets_open {
        return false;
    }
    let mut window_open = true;
    let mut changed = false;
    egui::Window::new("Manage item sets")
        .id(egui::Id::new("item-set-manage-window"))
        .open(&mut window_open)
        .resizable(false)
        .collapsible(false)
        .default_width(360.0)
        .show(ui.ctx(), |ui| {
            if character.item_sets.is_empty() {
                ui.weak("No saved item sets yet — save one from the row below.");
                // Falls through into the Import section so the user
                // can still paste a code into a fresh build.
            }
            // Snapshot so we can mutate `character.item_sets` inside the
            // loop (rename / clone / delete) without overlapping borrows.
            let entries: Vec<(usize, String)> = character
                .item_sets
                .iter()
                .enumerate()
                .map(|(i, s)| (i, s.name.clone()))
                .collect();
            let active = state.active_item_set_idx;
            for (idx, name) in entries {
                let is_active = active == Some(idx);
                let row_label = format_set_dropdown_label(&name, idx, is_active);
                ui.horizontal(|ui| {
                    ui.label(row_label);
                    // Issue #212 follow-up: interim non-drag reorder.
                    // The Up arrow is disabled on the first row and
                    // Down on the last so the affordance always renders
                    // — the grey state communicates the boundary
                    // without rearranging the row layout per index.
                    let is_first = idx == 0;
                    let is_last = idx + 1 >= character.item_sets.len();
                    if ui
                        .add_enabled(!is_first, egui::Button::new("▲").small())
                        .on_hover_text("Move this set up one position")
                        .clicked()
                    {
                        if let Some(new_idx) = move_item_set(character, idx, MoveDirection::Up) {
                            state.active_item_set_idx = shift_active_idx_after_swap(
                                state.active_item_set_idx,
                                idx,
                                new_idx,
                            );
                            changed = true;
                        }
                    }
                    if ui
                        .add_enabled(!is_last, egui::Button::new("▼").small())
                        .on_hover_text("Move this set down one position")
                        .clicked()
                    {
                        if let Some(new_idx) = move_item_set(character, idx, MoveDirection::Down) {
                            state.active_item_set_idx = shift_active_idx_after_swap(
                                state.active_item_set_idx,
                                idx,
                                new_idx,
                            );
                            changed = true;
                        }
                    }
                    if ui
                        .small_button("Load")
                        .on_hover_text("Activate this set")
                        .clicked()
                    {
                        if character.activate_item_set(idx) {
                            state.active_item_set_idx = Some(idx);
                            changed = true;
                        }
                    }
                    if ui
                        .small_button("Rename")
                        .on_hover_text("Rename this set")
                        .clicked()
                    {
                        state.item_set_rename = Some(ItemSetRenameState {
                            index: idx,
                            buffer: name.clone(),
                            last_error: None,
                        });
                    }
                    if ui
                        .small_button("Clone")
                        .on_hover_text("Duplicate this set in place")
                        .clicked()
                    {
                        let _ = clone_item_set(character, idx);
                    }
                    if ui
                        .small_button("Delete")
                        .on_hover_text("Remove this set")
                        .clicked()
                    {
                        if character.delete_item_set(idx) {
                            state.active_item_set_idx =
                                shift_active_idx_after_delete(state.active_item_set_idx, idx);
                        }
                    }
                    // Issue #212 (slice 2): per-row Export button.
                    // Copies an `MK2SET|...` code to the clipboard so
                    // the user can paste it into another build's
                    // Import box.
                    if ui
                        .small_button("Export")
                        .on_hover_text(
                            "Copy this set to the clipboard as an MK2SET share code. \
                             Paste it into another build's Import box to reproduce \
                             the loadout.",
                        )
                        .clicked()
                    {
                        if let Some(named) = character.item_sets.get(idx) {
                            match pob_engine::export_item_set(named) {
                                Ok(code) => ui.ctx().copy_text(code),
                                Err(e) => {
                                    state.item_set_import_error =
                                        Some(format!("Export failed: {e}"));
                                }
                            }
                        }
                    }
                });
            }
            ui.separator();
            // Issue #212 (slice 2): import a previously-exported set.
            // The user pastes an `MK2SET|...` code into the buffer;
            // Import decodes it and appends to `character.item_sets`.
            ui.label("Import set from clipboard:");
            ui.add(
                egui::TextEdit::multiline(&mut state.item_set_import_buffer)
                    .desired_width(f32::INFINITY)
                    .desired_rows(2)
                    .hint_text("Paste an MK2SET|… code here"),
            );
            ui.horizontal(|ui| {
                if ui.button("Import").clicked() {
                    match pob_engine::import_item_set(&state.item_set_import_buffer) {
                        Ok(named) => {
                            character.item_sets.push(named);
                            state.item_set_import_buffer.clear();
                            state.item_set_import_error = None;
                            changed = true;
                        }
                        Err(e) => {
                            state.item_set_import_error = Some(format!("Import failed: {e}"));
                        }
                    }
                }
                if ui.button("Clear").clicked() {
                    state.item_set_import_buffer.clear();
                    state.item_set_import_error = None;
                }
            });
            if let Some(err) = state.item_set_import_error.as_ref() {
                ui.colored_label(egui::Color32::from_rgb(0xDD, 0x00, 0x22), err);
            }
            ui.separator();
            if ui.button("Close").clicked() {
                state.manage_sets_open = false;
            }
        });
    if !window_open {
        state.manage_sets_open = false;
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

/// Issue #221 follow-up: what the equipped-item card reports back from a
/// click frame. `dot` is the 0-based socket dot index for a colour-cycle
/// click; `link` is the 0-based gap index for a link-toggle click. At
/// most one is `Some` per click — `draw_sockets` already enforces that
/// (dot hit takes precedence over gap hit). Tooltip callers ignore both.
#[derive(Debug, Default, Clone, Copy)]
struct SocketClick {
    dot: Option<usize>,
    link: Option<usize>,
}

/// Render the equipped-item card. Returns the socket click info (dot or
/// link) the user produced this frame — the caller mutates
/// `item.sockets` via [`crate::socket_renderer::apply_socket_cycle_at`]
/// or [`crate::socket_renderer::apply_socket_link_toggle_at`] and
/// persists. Tooltip callers can ignore the return.
fn render_item_summary(ui: &mut egui::Ui, item: &Item) -> SocketClick {
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
    let mut click = SocketClick::default();
    if !item.sockets.is_empty() {
        // Issue #221 (slice 1): visualise sockets as coloured dots with
        // link bars between sockets in the same group. Falls back to
        // the raw string if parsing produced nothing (defensive — the
        // parser is permissive, so this branch is mostly unreachable).
        //
        // Issue #221 follow-up: clicking a dot cycles its colour, and
        // clicking the gap between two dots toggles the link
        // (`-` ↔ ` `) — `SocketsResponse` carries both indices.
        let groups = pob_data::parse_socket_string(&item.sockets);
        if groups.is_empty() {
            ui.label(format!("Sockets: {}", item.sockets));
        } else {
            ui.horizontal(|ui| {
                ui.label("Sockets:");
                let resp = draw_sockets(ui, &groups, SocketLayoutConfig::default());
                resp.response.on_hover_text(
                    "Click a socket to cycle its colour (R → G → B → W).\n\
                     Click between two sockets to toggle the link.",
                );
                click.dot = resp.clicked_dot;
                click.link = resp.clicked_link;
            });
        }
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
    click
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
                    variant_list: None,
                })
                .collect(),
            sockets: String::new(),
            raw: String::new(),
            corrupted: false,
            mirrored: false,
            variants: Vec::new(),
            variant: None,
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
            variants: Vec::new(),
            variant: None,
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

    // ─── Issue #212 (slice 1, data layer): item-set CRUD helpers ────────
    //
    // These exercise `rename_item_set` and `clone_item_set` in isolation
    // — no UI in the loop. Each test seeds a `Character` with a couple of
    // named sets via the existing `save_item_set` engine API so we don't
    // depend on UI plumbing.

    use pob_engine::ClassRef;

    fn seeded_character(names: &[&str]) -> Character {
        let mut c = Character::new(ClassRef::scion(), 1);
        // Use a distinct active item per saved set so we can later assert
        // that `clone_item_set` actually copies the *source*'s items, not
        // whatever happens to be live on `character.items`.
        for (i, name) in names.iter().enumerate() {
            // Make each set's "items" distinguishable: stash a different
            // base_name into a slot before saving.
            let item = Item {
                name: String::new(),
                base_name: format!("Marker-{i}"),
                rarity: Rarity::Normal,
                item_level: 1,
                quality: 0,
                tags: HashSet::default(),
                mod_lines: Vec::new(),
                sockets: String::new(),
                raw: String::new(),
                corrupted: false,
                mirrored: false,
                variants: Vec::new(),
                variant: None,
            };
            c.items.equip(Slot::Amulet, item);
            c.save_item_set((*name).to_owned());
        }
        c
    }

    #[test]
    fn move_item_set_up_swaps_with_previous() {
        // Mid-list down→up move. Returned index points at the new
        // position of the moved entry; the displaced entry now sits
        // at the original index.
        let mut c = seeded_character(&["A", "B", "C"]);
        let new_idx = move_item_set(&mut c, 1, MoveDirection::Up).expect("can move up");
        assert_eq!(new_idx, 0);
        let names: Vec<&str> = c.item_sets.iter().map(|s| s.name.as_str()).collect();
        assert_eq!(names, vec!["B", "A", "C"]);
    }

    #[test]
    fn move_item_set_down_swaps_with_next() {
        let mut c = seeded_character(&["A", "B", "C"]);
        let new_idx = move_item_set(&mut c, 1, MoveDirection::Down).expect("can move down");
        assert_eq!(new_idx, 2);
        let names: Vec<&str> = c.item_sets.iter().map(|s| s.name.as_str()).collect();
        assert_eq!(names, vec!["A", "C", "B"]);
    }

    #[test]
    fn move_item_set_up_at_top_is_noop() {
        // First entry can't move up. The list stays put and the helper
        // returns `None` so the UI can grey out the button.
        let mut c = seeded_character(&["A", "B"]);
        assert!(move_item_set(&mut c, 0, MoveDirection::Up).is_none());
        let names: Vec<&str> = c.item_sets.iter().map(|s| s.name.as_str()).collect();
        assert_eq!(names, vec!["A", "B"]);
    }

    #[test]
    fn move_item_set_down_at_bottom_is_noop() {
        let mut c = seeded_character(&["A", "B"]);
        assert!(move_item_set(&mut c, 1, MoveDirection::Down).is_none());
        let names: Vec<&str> = c.item_sets.iter().map(|s| s.name.as_str()).collect();
        assert_eq!(names, vec!["A", "B"]);
    }

    #[test]
    fn move_item_set_out_of_range_is_noop() {
        // Defensive against a stale click after a delete shrank the
        // list — same shape as the rename / clone OutOfRange cases.
        let mut c = seeded_character(&["A"]);
        assert!(move_item_set(&mut c, 5, MoveDirection::Up).is_none());
        assert!(move_item_set(&mut c, 5, MoveDirection::Down).is_none());
        // Empty list — any index is out of range.
        let mut empty = seeded_character(&[]);
        assert!(move_item_set(&mut empty, 0, MoveDirection::Up).is_none());
    }

    #[test]
    fn rename_item_set_updates_name_in_place() {
        let mut c = seeded_character(&["A", "B"]);
        assert_eq!(rename_item_set(&mut c, 0, "A-renamed"), Ok(()));
        assert_eq!(c.item_sets[0].name, "A-renamed");
        assert_eq!(c.item_sets[1].name, "B");
    }

    #[test]
    fn rename_item_set_trims_whitespace() {
        let mut c = seeded_character(&["A"]);
        assert_eq!(rename_item_set(&mut c, 0, "   spaced   "), Ok(()));
        assert_eq!(c.item_sets[0].name, "spaced");
    }

    #[test]
    fn rename_item_set_rejects_empty_name() {
        let mut c = seeded_character(&["A"]);
        assert_eq!(
            rename_item_set(&mut c, 0, "   "),
            Err(ItemSetOpError::EmptyName)
        );
        // Original name preserved.
        assert_eq!(c.item_sets[0].name, "A");
    }

    #[test]
    fn rename_item_set_rejects_out_of_range() {
        let mut c = seeded_character(&["A"]);
        assert_eq!(
            rename_item_set(&mut c, 5, "x"),
            Err(ItemSetOpError::OutOfRange)
        );
    }

    #[test]
    fn rename_item_set_rejects_duplicate_name() {
        let mut c = seeded_character(&["A", "B"]);
        assert_eq!(
            rename_item_set(&mut c, 0, "B"),
            Err(ItemSetOpError::DuplicateName)
        );
        // Both sets unchanged.
        assert_eq!(c.item_sets[0].name, "A");
        assert_eq!(c.item_sets[1].name, "B");
    }

    #[test]
    fn rename_item_set_to_self_is_noop_success() {
        // Allows the rename UI to commit without distinguishing
        // "user typed the same thing" from "real rename" — important
        // because the existence-check would otherwise flag the entry
        // itself as a duplicate.
        let mut c = seeded_character(&["A"]);
        assert_eq!(rename_item_set(&mut c, 0, "A"), Ok(()));
        assert_eq!(c.item_sets[0].name, "A");
    }

    #[test]
    fn clone_item_set_inserts_copy_after_source() {
        let mut c = seeded_character(&["A", "B"]);
        let new_idx = clone_item_set(&mut c, 0).expect("clone ok");
        assert_eq!(new_idx, 1);
        assert_eq!(c.item_sets.len(), 3);
        assert_eq!(c.item_sets[0].name, "A");
        assert_eq!(c.item_sets[1].name, "A (copy)");
        assert_eq!(c.item_sets[2].name, "B");
        // The clone's items match the source's items, not the active items.
        assert_eq!(
            c.item_sets[1].items.get(Slot::Amulet).map(|i| &i.base_name),
            c.item_sets[0].items.get(Slot::Amulet).map(|i| &i.base_name),
        );
    }

    #[test]
    fn clone_item_set_preserves_active_items() {
        // Regression guard for the implementation trick of routing through
        // `save_item_set`: we must restore `character.items` afterwards.
        let mut c = seeded_character(&["A"]);
        // Replace the active items with a known marker.
        c.items = ItemSet::new();
        let marker = Item {
            name: String::new(),
            base_name: "ActiveMarker".to_owned(),
            rarity: Rarity::Normal,
            item_level: 1,
            quality: 0,
            tags: HashSet::default(),
            mod_lines: Vec::new(),
            sockets: String::new(),
            raw: String::new(),
            corrupted: false,
            mirrored: false,
            variants: Vec::new(),
            variant: None,
        };
        c.items.equip(Slot::Helmet, marker);

        let _ = clone_item_set(&mut c, 0).expect("clone ok");

        assert_eq!(
            c.items.get(Slot::Helmet).map(|i| i.base_name.as_str()),
            Some("ActiveMarker"),
            "active items must survive a clone"
        );
        assert!(
            c.items.get(Slot::Amulet).is_none(),
            "active items must NOT have been overwritten by the cloned set"
        );
    }

    #[test]
    fn clone_item_set_avoids_name_collision() {
        let mut c = seeded_character(&["A", "A (copy)"]);
        let new_idx = clone_item_set(&mut c, 0).expect("clone ok");
        assert_eq!(new_idx, 1);
        assert_eq!(c.item_sets[1].name, "A (copy 2)");
        // Existing "A (copy)" pushed down by the insert.
        assert_eq!(c.item_sets[2].name, "A (copy)");
    }

    #[test]
    fn clone_item_set_rejects_out_of_range() {
        let mut c = seeded_character(&["A"]);
        assert_eq!(clone_item_set(&mut c, 9), Err(ItemSetOpError::OutOfRange));
        // No spurious set added.
        assert_eq!(c.item_sets.len(), 1);
    }

    // ─── Issue #222: switcher dropdown + manage popup helpers ────────────
    //
    // The two pure helpers (`format_set_dropdown_label` and
    // `shift_active_idx_after_delete`) moved to `crate::set_switcher` so
    // the future skill-set / config-set switchers can share them. Their
    // unit tests moved with them — see `set_switcher::tests`. The
    // `ItemsTabState` default-state guard stays here because it's
    // tab-specific. The tooltip helper below also lives in `items_tab`
    // because the slot- and mod-count summary is specific to item sets.

    fn tooltip_set(name: &str, slots: &[(Slot, &[&str])]) -> NamedItemSet {
        let mut items = ItemSet::new();
        for (slot, mods) in slots {
            let mod_lines: Vec<ModLine> = mods
                .iter()
                .map(|line| ModLine::new(*line, ModSection::Explicit))
                .collect();
            items.equip(
                *slot,
                Item {
                    name: String::new(),
                    base_name: "Base".to_owned(),
                    rarity: Rarity::Normal,
                    item_level: 1,
                    quality: 0,
                    tags: HashSet::default(),
                    mod_lines,
                    sockets: String::new(),
                    raw: String::new(),
                    corrupted: false,
                    mirrored: false,
                    variants: Vec::new(),
                    variant: None,
                },
            );
        }
        NamedItemSet {
            name: name.to_owned(),
            items,
        }
    }

    #[test]
    fn tooltip_lines_for_empty_set_show_zero_counts() {
        // A freshly-saved blank set should still produce a usable hover
        // body — the user might be parking the slot for later.
        let set = tooltip_set("Empty", &[]);
        let lines = format_item_set_tooltip_lines(&set);
        assert_eq!(lines[0], "Empty");
        assert_eq!(lines[1], "Slots filled: 0");
        assert_eq!(lines[2], "Mod lines: 0");
    }

    #[test]
    fn tooltip_lines_count_each_filled_slot() {
        // Two items, no mods: slot count tracks the populated slots and
        // the mod-count line stays at zero so users can tell at a glance
        // that the set is "two whites".
        let set = tooltip_set("Levelling", &[(Slot::Helmet, &[]), (Slot::Amulet, &[])]);
        let lines = format_item_set_tooltip_lines(&set);
        assert_eq!(lines[1], "Slots filled: 2");
        assert_eq!(lines[2], "Mod lines: 0");
    }

    #[test]
    fn tooltip_lines_sum_mod_lines_across_items() {
        // Mod count is summed across every equipped item — that's the
        // PoB-side "how much stuff is on this set" signal, and it's the
        // one number that survives the slot-set being identical.
        let set = tooltip_set(
            "Endgame",
            &[
                (Slot::Helmet, &["+50 Life", "20% Resist"]),
                (Slot::Amulet, &["+1 Skill"]),
            ],
        );
        let lines = format_item_set_tooltip_lines(&set);
        assert_eq!(lines[1], "Slots filled: 2");
        assert_eq!(lines[2], "Mod lines: 3");
    }

    #[test]
    fn tooltip_lines_substitute_unnamed_for_blank_name() {
        // Blank names would render as an empty first line in the hover
        // tooltip and look like a layout bug. Substitute the same
        // placeholder the closed combo box uses.
        let set = tooltip_set("   ", &[]);
        let lines = format_item_set_tooltip_lines(&set);
        assert_eq!(lines[0], "(unnamed)");
    }

    #[test]
    fn switcher_state_defaults_are_closed_and_empty() {
        // Cold-open of the Items tab must NOT pop the manage window or
        // claim a switcher selection — both should be opt-in.
        let s = ItemsTabState::default();
        assert_eq!(s.active_item_set_idx, None);
        assert!(!s.manage_sets_open);
    }

    // ─── Issue #207 (panel slice): top-contributors panel ────────────────

    #[test]
    fn top_contributors_panel_default_state_is_closed() {
        // The collapsing header is opt-in: opening the Items tab cold
        // must NOT trigger the M+1-perform-call-per-equipped-item
        // ranking pass. Guards against an accidental flip of the
        // `default_open` flag in a future refactor.
        let s = ItemsTabState::default();
        assert!(!s.top_contributors_open);
    }

    #[test]
    fn sort_and_format_contributors_combined_uses_max_axis() {
        // Mirrors the previous default. A mod line that's pure DPS
        // (high dps, zero ehp) and one that's pure EHP (zero dps,
        // high ehp) of equal magnitude tie at the top — `Combined`
        // collapses to `max(dps, ehp)`.
        let scores = vec![
            ItemModlineScore {
                slot: pob_data::Slot::Helmet,
                mod_index: 0,
                mod_line: "small-defence".to_owned(),
                dps_delta: 0.0,
                ehp_delta: 50.0,
            },
            ItemModlineScore {
                slot: pob_data::Slot::Helmet,
                mod_index: 1,
                mod_line: "big-offence".to_owned(),
                dps_delta: 200.0,
                ehp_delta: 0.0,
            },
        ];
        let lines = sort_and_format_contributors(
            scores,
            crate::node_power_heatmap::HeatmapStat::Combined,
            10,
        );
        assert_eq!(lines.len(), 2);
        // Top row is the larger of the two `max`-collapsed values.
        assert!(
            lines[0].contains("big-offence"),
            "Combined axis should rank +200 DPS above +50 EHP, got {lines:?}",
        );
    }

    #[test]
    fn sort_and_format_contributors_dps_prefers_dps_lines() {
        // A defence-only line scores 0 on the DPS axis and falls below
        // every DPS-positive line.
        let scores = vec![
            ItemModlineScore {
                slot: pob_data::Slot::Helmet,
                mod_index: 0,
                mod_line: "tiny-dps".to_owned(),
                dps_delta: 1.0,
                ehp_delta: 0.0,
            },
            ItemModlineScore {
                slot: pob_data::Slot::Helmet,
                mod_index: 1,
                mod_line: "huge-defence".to_owned(),
                dps_delta: 0.0,
                ehp_delta: 999.0,
            },
        ];
        let lines =
            sort_and_format_contributors(scores, crate::node_power_heatmap::HeatmapStat::Dps, 10);
        assert!(
            lines[0].contains("tiny-dps"),
            "DPS axis must rank any DPS-positive line above an EHP-only one, got {lines:?}",
        );
    }

    #[test]
    fn sort_and_format_contributors_ehp_prefers_ehp_lines() {
        // The mirror of the DPS-axis case.
        let scores = vec![
            ItemModlineScore {
                slot: pob_data::Slot::Helmet,
                mod_index: 0,
                mod_line: "tiny-defence".to_owned(),
                dps_delta: 0.0,
                ehp_delta: 1.0,
            },
            ItemModlineScore {
                slot: pob_data::Slot::Helmet,
                mod_index: 1,
                mod_line: "huge-dps".to_owned(),
                dps_delta: 999.0,
                ehp_delta: 0.0,
            },
        ];
        let lines =
            sort_and_format_contributors(scores, crate::node_power_heatmap::HeatmapStat::Ehp, 10);
        assert!(
            lines[0].contains("tiny-defence"),
            "EHP axis must rank any EHP-positive line above a DPS-only one, got {lines:?}",
        );
    }

    #[test]
    fn sort_and_format_contributors_truncates_to_top_n() {
        // `format_top_contributors` already truncates; verify the
        // wrapper honours that contract end-to-end after the sort step.
        let scores: Vec<ItemModlineScore> = (0i32..5)
            .map(|i| ItemModlineScore {
                slot: pob_data::Slot::Helmet,
                mod_index: i as usize,
                mod_line: format!("line-{i}"),
                dps_delta: f64::from(i),
                ehp_delta: 0.0,
            })
            .collect();
        let lines =
            sort_and_format_contributors(scores, crate::node_power_heatmap::HeatmapStat::Dps, 2);
        assert_eq!(lines.len(), 2);
        assert!(lines[0].contains("line-4"));
        assert!(lines[1].contains("line-3"));
    }

    fn modline_score(slot: pob_data::Slot, line: &str, dps: f64, ehp: f64) -> ItemModlineScore {
        ItemModlineScore {
            slot,
            mod_index: 0,
            mod_line: line.to_owned(),
            dps_delta: dps,
            ehp_delta: ehp,
        }
    }

    #[test]
    fn aggregate_contributors_by_slot_sums_per_slot() {
        // Two scores on Helmet, one on Belt — Helmet's totals should
        // be the sum of its rows; Belt is a singleton.
        let scores = vec![
            modline_score(pob_data::Slot::Helmet, "a", 10.0, 5.0),
            modline_score(pob_data::Slot::Helmet, "b", 20.0, 15.0),
            modline_score(pob_data::Slot::Belt, "c", 100.0, 0.0),
        ];
        let agg = aggregate_contributors_by_slot(
            &scores,
            crate::node_power_heatmap::HeatmapStat::Combined,
        );
        let helmet = agg
            .iter()
            .find(|(s, _, _)| *s == pob_data::Slot::Helmet)
            .expect("helmet entry");
        assert!(
            (helmet.1 - 30.0).abs() < 1e-9,
            "summed DPS, got {}",
            helmet.1
        );
        assert!(
            (helmet.2 - 20.0).abs() < 1e-9,
            "summed EHP, got {}",
            helmet.2
        );
        let belt = agg
            .iter()
            .find(|(s, _, _)| *s == pob_data::Slot::Belt)
            .expect("belt entry");
        assert!((belt.1 - 100.0).abs() < 1e-9);
    }

    #[test]
    fn aggregate_contributors_by_slot_sorts_descending_under_dps_axis() {
        // DPS axis must put the bigger-DPS slot first regardless of
        // EHP totals.
        let scores = vec![
            modline_score(pob_data::Slot::Helmet, "a", 10.0, 100.0),
            modline_score(pob_data::Slot::Belt, "b", 500.0, 0.0),
            modline_score(pob_data::Slot::Boots, "c", 50.0, 0.0),
        ];
        let agg =
            aggregate_contributors_by_slot(&scores, crate::node_power_heatmap::HeatmapStat::Dps);
        let order: Vec<pob_data::Slot> = agg.iter().map(|(s, _, _)| *s).collect();
        assert_eq!(
            order,
            vec![
                pob_data::Slot::Belt,
                pob_data::Slot::Boots,
                pob_data::Slot::Helmet
            ]
        );
    }

    #[test]
    fn aggregate_contributors_by_slot_sorts_descending_under_ehp_axis() {
        // Mirror: EHP axis prioritises Helmet (which has the biggest
        // EHP total) over the DPS-dominated Belt.
        let scores = vec![
            modline_score(pob_data::Slot::Helmet, "a", 10.0, 100.0),
            modline_score(pob_data::Slot::Belt, "b", 500.0, 0.0),
        ];
        let agg =
            aggregate_contributors_by_slot(&scores, crate::node_power_heatmap::HeatmapStat::Ehp);
        let order: Vec<pob_data::Slot> = agg.iter().map(|(s, _, _)| *s).collect();
        assert_eq!(order, vec![pob_data::Slot::Helmet, pob_data::Slot::Belt]);
    }

    #[test]
    fn aggregate_contributors_by_slot_empty_input_returns_empty() {
        let agg =
            aggregate_contributors_by_slot(&[], crate::node_power_heatmap::HeatmapStat::Combined);
        assert!(agg.is_empty());
    }

    #[test]
    fn format_slot_contributions_carries_slot_label_and_signed_totals() {
        let agg = vec![
            (pob_data::Slot::Belt, 500.0, 0.0),
            (pob_data::Slot::Helmet, -10.0, 100.0),
        ];
        let lines = format_slot_contributions(&agg);
        assert_eq!(lines.len(), 2);
        assert!(lines[0].contains("+500"));
        assert!(lines[0].ends_with("Belt"));
        assert!(lines[1].contains("-10"));
        assert!(lines[1].contains("+100"));
        assert!(lines[1].ends_with("Helmet"));
    }

    #[test]
    fn top_contributors_panel_empty_character_is_empty() {
        // No equipped items → no mod lines to score → empty result.
        // Verifies the panel doesn't panic when wired into a fresh
        // build, and that the empty-state branch the UI renders the
        // "(no equipped mod lines to score)" label for is reachable.
        use ahash::HashMap as AHashMap;
        use pob_data::{TreeConstants, TreePoints};
        let c = Character::new(ClassRef::scion(), 1);
        let tree = PassiveTree {
            version: "test".into(),
            tree: "test".into(),
            classes: vec![],
            groups: AHashMap::default(),
            nodes: AHashMap::default(),
            jewel_slots: vec![],
            min_x: 0,
            min_y: 0,
            max_x: 0,
            max_y: 0,
            constants: TreeConstants {
                skills_per_orbit: vec![],
                orbit_radii: vec![],
                classes: AHashMap::default(),
                character_attributes: AHashMap::default(),
                pss_centre_inner_radius: None,
            },
            points: TreePoints::default(),
        };
        let skills = SkillRegistry::default();
        let scores = collect_item_modline_scores(&c, &tree, &skills, None);
        assert!(scores.is_empty(), "no equipped items → no mod-line scores");
        let lines = sort_and_format_contributors(
            scores,
            crate::node_power_heatmap::HeatmapStat::default(),
            10,
        );
        assert!(lines.is_empty());
    }
}
