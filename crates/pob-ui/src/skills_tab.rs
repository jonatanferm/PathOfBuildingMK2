//! Skills tab — manage skill gem groups and pick the main active skill.
//!
//! Layout:
//! - Left: socket-group list with add/remove buttons
//! - Middle: gems within the selected group
//! - Right: skill catalog (filterable) + level/quality sliders for the
//!   currently-selected gem
//!
//! Catalog filtering follows PoB's `Classes/GemSelectControl.lua` shape: color
//! chips (R/G/B/W) by gem requirement, type chips (active/support/awakened/vaal)
//! by name + flags, and a free-text search that matches name and tag keywords.
//! Hide-legacy is a coarse heuristic over skill ids/names — we don't have a
//! dedicated `removed` flag in the extracted data yet (see PR notes).

use eframe::egui;
use pob_data::Skill;
use pob_engine::character::SocketGroup;
use pob_engine::skill::base_skill_id;
use pob_engine::{Character, MainSkill, QualityId, SkillRegistry};

use crate::color_codes;

/// Issue #209 follow-up: clear every gem-picker filter facet back to
/// the cold-open default — search text, color chips, type chips, tag
/// chips. Mirrors the Items-tab `BrowseFilter::reset` rule. The
/// `hide_legacy` and `default_level_on_add` preferences are sticky
/// across resets, so a power user who configured their preferred
/// catalogue view doesn't lose it.
///
/// Returns `true` iff at least one facet was non-default — the caller
/// can use the same value to decide whether the Reset button should
/// have been enabled in the first place.
pub fn reset_gem_picker_filters(state: &mut SkillsTabState) -> bool {
    let any = gem_picker_filters_active(state);
    if !any {
        return false;
    }
    state.filter.clear();
    state.colors = ColorFilter::default();
    state.tags = TagFilter::default();
    state.types = TypeFilter::default();
    true
}

/// Issue #209 follow-up: whether at least one gem-picker filter facet
/// is active. Drives the enable-state of the Reset button so a
/// cold-open click is inert.
#[must_use]
pub fn gem_picker_filters_active(state: &SkillsTabState) -> bool {
    !state.filter.trim().is_empty() || state.colors.any() || state.tags.any() || state.types.any()
}

/// Issue #214 follow-up: bulk-toggle every socket group's `enabled`
/// flag. Returns `true` when at least one group's flag changed. Pure
/// — no I/O, no recompute (the caller flips `recompute = true` so the
/// engine re-runs on the next frame).
pub fn enable_all_socket_groups(character: &mut pob_engine::Character) -> bool {
    let mut changed = false;
    for g in &mut character.skill_groups {
        if !g.enabled {
            g.enabled = true;
            changed = true;
        }
    }
    changed
}

/// Issue #214 follow-up: bulk-toggle every socket group's `enabled`
/// flag off. Reverses [`enable_all_socket_groups`] — useful when the
/// user wants to start A/B-ing from a "nothing on" baseline rather
/// than soloing one group at a time. Returns `true` iff at least one
/// group changed.
pub fn disable_all_socket_groups(character: &mut pob_engine::Character) -> bool {
    let mut changed = false;
    for g in &mut character.skill_groups {
        if g.enabled {
            g.enabled = false;
            changed = true;
        }
    }
    changed
}

/// Issue #214 follow-up: how many of the saved socket groups are
/// currently contributing their gems, vs. how many exist in total.
/// Drives the header-row chip — same pattern as the Party tab's
/// "N of M active" chip.
#[must_use]
pub fn count_active_socket_groups(character: &pob_engine::Character) -> (usize, usize) {
    let total = character.skill_groups.len();
    let active = character.skill_groups.iter().filter(|g| g.enabled).count();
    (active, total)
}

/// Issue #214 follow-up: leave only the group at `idx` enabled,
/// disabling every other group. Returns `true` when at least one
/// group's flag changed. `idx` out of range is a no-op (returns
/// `false`).
///
/// Pulled out as a pure helper so the "Solo" button in the group
/// list has a unit-test home — the multi-target mutation is easy
/// to get wrong (off-by-one, leaving the target disabled).
pub fn solo_socket_group_at(character: &mut pob_engine::Character, idx: usize) -> bool {
    if idx >= character.skill_groups.len() {
        return false;
    }
    let mut changed = false;
    for (i, g) in character.skill_groups.iter_mut().enumerate() {
        let want = i == idx;
        if g.enabled != want {
            g.enabled = want;
            changed = true;
        }
    }
    changed
}

/// Issue #214 (slice 3): payload carried by a gem drag-source. Carries
/// both the source group and the source gem index so the drop target
/// can route to either `move_gem` (same group) or
/// `move_gem_across_groups` (cross group). A dedicated struct keeps the
/// payload type distinct from the `usize` payload used by group-row
/// drag-and-drop reorder (slice 2 / PR #369), so the two systems can
/// coexist on the same group-list row via two payload-typed drop checks.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct GemDragPayload {
    pub group_idx: usize,
    pub gem_idx: usize,
}

/// Color/requirement filter for the gem picker. Each chip is a tri-state of
/// "include" — when *all* chips are off the picker shows everything.
#[derive(Debug, Clone, Copy, Default)]
pub struct ColorFilter {
    pub red: bool,
    pub green: bool,
    pub blue: bool,
    pub white: bool,
}

impl ColorFilter {
    pub fn any(self) -> bool {
        self.red || self.green || self.blue || self.white
    }
}

/// Tag filter — keyword chips PoB exposes as quick narrowing. We map each chip
/// to a predicate over `Skill::base_flags` / name; "Cold/Fire/Lightning/Chaos
/// /Physical" check the skill's `stats` list (the same probe used by the calc
/// engine in `skill_damage_element`).
#[derive(Debug, Clone, Copy, Default)]
pub struct TagFilter {
    pub spell: bool,
    pub attack: bool,
    pub aura: bool,
    pub herald: bool,
    pub fire: bool,
    pub cold: bool,
    pub lightning: bool,
    pub chaos: bool,
    pub physical: bool,
}

impl TagFilter {
    pub fn any(self) -> bool {
        self.spell
            || self.attack
            || self.aura
            || self.herald
            || self.fire
            || self.cold
            || self.lightning
            || self.chaos
            || self.physical
    }
}

/// Gem-type chips. PoB's picker has discrete buttons for "Awakened",
/// "Exceptional" (Empower/Enhance/Enlighten), and "Vaal".
#[derive(Debug, Clone, Copy, Default)]
pub struct TypeFilter {
    pub active: bool,
    pub support: bool,
    pub awakened: bool,
    pub exceptional: bool,
    pub vaal: bool,
}

impl TypeFilter {
    pub fn any(self) -> bool {
        self.active || self.support || self.awakened || self.exceptional || self.vaal
    }
}

pub struct SkillsTabState {
    pub filter: String,
    pub colors: ColorFilter,
    pub tags: TagFilter,
    pub types: TypeFilter,
    pub hide_legacy: bool,
    pub default_level_on_add: bool,
    pub selected_group: usize,
    pub selected_gem: usize,
    pub catalog_open: bool,
}

impl Default for SkillsTabState {
    fn default() -> Self {
        Self {
            filter: String::new(),
            colors: ColorFilter::default(),
            tags: TagFilter::default(),
            types: TypeFilter::default(),
            hide_legacy: true,
            default_level_on_add: true,
            selected_group: 0,
            selected_gem: 0,
            catalog_open: false,
        }
    }
}

/// "Default" level for a skill — what the catalog button auto-fills the level
/// field with. Mirrors PoB's `naturalMaxLevel` semantics: highest gem level
/// present in the skill's `levels` table, capped at 20 (i.e. don't auto-corrupt
/// to 21+ levels for awakened gems unless the data goes there). Returns `None`
/// when no level data is available.
pub fn default_gem_level(skill: &Skill) -> Option<u32> {
    if skill.levels.is_empty() {
        return None;
    }
    let max = skill.levels.len() as u32;
    Some(max.clamp(1, 20))
}

/// Crude legacy detection. PoB flags removed/legacy gems explicitly in
/// `Data/Gems.lua` via `naturalMaxLevel = 0` and the `Removed` skill-type
/// alongside id-prefix conventions ("Removed" / "Vaal" duplicates) — but the
/// extractor doesn't carry that flag onto `Skill`. We approximate by checking
/// the skill id and name for the "Removed" prefix the data already uses for
/// removed-from-game skills. Better data plumbing is tracked as a follow-up.
pub fn is_legacy_skill(id: &str, skill: &Skill) -> bool {
    if skill.levels.is_empty() {
        return true;
    }
    let lid = id.to_ascii_lowercase();
    if lid.starts_with("removed") || lid.contains("removed_") {
        return true;
    }
    let lname = skill.name.to_ascii_lowercase();
    lname.starts_with("removed ") || lname.starts_with("[removed]")
}

/// Iterate human-readable tag tokens for a skill — what the search box
/// matches and what the per-row tag-line in the picker shows. Order matters
/// only for the rendered string; matching is case-insensitive.
pub fn skill_tag_tokens(skill: &Skill) -> Vec<&'static str> {
    let mut out: Vec<&'static str> = Vec::new();
    let f = &skill.base_flags;
    if f.get("spell").copied().unwrap_or(false) {
        out.push("Spell");
    }
    if f.get("attack").copied().unwrap_or(false) {
        out.push("Attack");
    }
    if f.get("aura").copied().unwrap_or(false) {
        out.push("Aura");
    }
    if f.get("herald").copied().unwrap_or(false) {
        out.push("Herald");
    }
    if f.get("area").copied().unwrap_or(false) {
        out.push("AoE");
    }
    if f.get("projectile").copied().unwrap_or(false) {
        out.push("Projectile");
    }
    if f.get("melee").copied().unwrap_or(false) {
        out.push("Melee");
    }
    if f.get("chaining").copied().unwrap_or(false) {
        out.push("Chaining");
    }
    if f.get("minion").copied().unwrap_or(false) {
        out.push("Minion");
    }
    // Damage-type keywords from the per-skill `stats` list (the same probe
    // the calc engine uses to find the dominant damage element).
    let mut elements = [false; 5]; // fire, cold, lightning, chaos, physical
    for stat in &skill.stats {
        let s = stat.as_str();
        if s.contains("fire") {
            elements[0] = true;
        }
        if s.contains("cold") {
            elements[1] = true;
        }
        if s.contains("lightning") {
            elements[2] = true;
        }
        if s.contains("chaos") {
            elements[3] = true;
        }
        if s.contains("physical") {
            elements[4] = true;
        }
    }
    if elements[0] {
        out.push("Fire");
    }
    if elements[1] {
        out.push("Cold");
    }
    if elements[2] {
        out.push("Lightning");
    }
    if elements[3] {
        out.push("Chaos");
    }
    if elements[4] {
        out.push("Physical");
    }
    out
}

fn skill_color(skill: &Skill) -> char {
    match skill.color {
        1 => 'R',
        2 => 'G',
        3 => 'B',
        _ => 'W',
    }
}

fn matches_color(skill: &Skill, f: ColorFilter) -> bool {
    if !f.any() {
        return true;
    }
    match skill_color(skill) {
        'R' => f.red,
        'G' => f.green,
        'B' => f.blue,
        'W' => f.white,
        _ => true,
    }
}

fn matches_tags(skill: &Skill, t: TagFilter) -> bool {
    if !t.any() {
        return true;
    }
    let f = &skill.base_flags;
    let has = |k: &str| f.get(k).copied().unwrap_or(false);
    let mut elements = [false; 5];
    for stat in &skill.stats {
        let s = stat.as_str();
        if s.contains("fire") {
            elements[0] = true;
        }
        if s.contains("cold") {
            elements[1] = true;
        }
        if s.contains("lightning") {
            elements[2] = true;
        }
        if s.contains("chaos") {
            elements[3] = true;
        }
        if s.contains("physical") {
            elements[4] = true;
        }
    }
    // Each enabled chip must match (AND across selected chips).
    if t.spell && !has("spell") {
        return false;
    }
    if t.attack && !has("attack") {
        return false;
    }
    if t.aura && !has("aura") {
        return false;
    }
    if t.herald && !has("herald") {
        return false;
    }
    if t.fire && !elements[0] {
        return false;
    }
    if t.cold && !elements[1] {
        return false;
    }
    if t.lightning && !elements[2] {
        return false;
    }
    if t.chaos && !elements[3] {
        return false;
    }
    if t.physical && !elements[4] {
        return false;
    }
    true
}

fn matches_type(id: &str, skill: &Skill, t: TypeFilter) -> bool {
    if !t.any() {
        return true;
    }
    let lname = skill.name.to_ascii_lowercase();
    let lid = id.to_ascii_lowercase();
    let is_support = skill.support;
    let is_awakened = lname.starts_with("awakened ") || lid.contains("awakened");
    let is_vaal = lname.starts_with("vaal ") || lid.starts_with("vaal");
    // Exceptional supports = Empower / Enhance / Enlighten — PoB's terminology.
    let is_exceptional = matches!(
        lname.split_whitespace().next().unwrap_or(""),
        "empower" | "enhance" | "enlighten"
    );
    if t.active && (is_support) {
        return false;
    }
    if t.support && !is_support {
        return false;
    }
    if t.awakened && !is_awakened {
        return false;
    }
    if t.exceptional && !is_exceptional {
        return false;
    }
    if t.vaal && !is_vaal {
        return false;
    }
    true
}

fn matches_search(skill: &Skill, q: &str) -> bool {
    if q.is_empty() {
        return true;
    }
    if skill.name.to_ascii_lowercase().contains(q) {
        return true;
    }
    for tok in skill_tag_tokens(skill) {
        if tok.to_ascii_lowercase().contains(q) {
            return true;
        }
    }
    false
}

/// Pure predicate the catalog list applies. Exposed so it can be unit-tested
/// without spinning up an egui context.
pub fn passes_filters(id: &str, skill: &Skill, state: &SkillsTabState, query_lc: &str) -> bool {
    if state.hide_legacy && is_legacy_skill(id, skill) {
        return false;
    }
    if !matches_color(skill, state.colors) {
        return false;
    }
    if !matches_tags(skill, state.tags) {
        return false;
    }
    if !matches_type(id, skill, state.types) {
        return false;
    }
    if !matches_search(skill, query_lc) {
        return false;
    }
    true
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
            // Issue #214 follow-up: bulk-toggle row. "Solo" leaves only
            // the selected group enabled so the user can A/B a single
            // gem-link's contribution without manually flipping every
            // other group; "Enable all" reverses it.
            if !character.skill_groups.is_empty() {
                ui.horizontal(|ui| {
                    let solo_target = state.selected_group;
                    let can_solo = solo_target < character.skill_groups.len();
                    if ui
                        .add_enabled(can_solo, egui::Button::new("Solo").small())
                        .on_hover_text(
                            "Disable every group except the currently selected one. \
                             Useful for A/B-ing one gem-link's contribution against the \
                             rest of the build.",
                        )
                        .clicked()
                        && solo_socket_group_at(character, solo_target)
                    {
                        changed = true;
                    }
                    let (active, total) = count_active_socket_groups(character);
                    let any_off = active < total;
                    let any_on = active > 0;
                    if ui
                        .add_enabled(any_off, egui::Button::new("Enable all"))
                        .on_hover_text("Re-enable every group — undoes a `Solo` click.")
                        .clicked()
                        && enable_all_socket_groups(character)
                    {
                        changed = true;
                    }
                    if ui
                        .add_enabled(any_on, egui::Button::new("Disable all"))
                        .on_hover_text(
                            "Turn off every group's gem contribution. Lets a user \
                             start from a nothing-on baseline when A/B-ing.",
                        )
                        .clicked()
                        && disable_all_socket_groups(character)
                    {
                        changed = true;
                    }
                    ui.weak(format!("{active} of {total} on"));
                });
            }
            ui.separator();
            let mut to_remove: Option<usize> = None;
            // Issue #214 (slice 2): drag-reorder socket groups. Each row
            // is wrapped in a dnd_drop_zone, with the "≡" handle on the
            // left as the dnd_drag_source. Source payload is the source
            // group index; the drop zone reads it back as
            // `(from, idx_at_drop)` and feeds `move_skill_group`.
            // Mirrors the gem-row wiring from slice 1 (PR #368).
            let mut pending_group_move: Option<(usize, usize)> = None;
            // Issue #214 (slice 3): a gem dragged from anywhere can be
            // dropped on a group-row to move it INTO that group. The
            // group-row's `dnd_drop_zone` already accepts a `usize` for
            // group reorder; we additionally check the same response for
            // a `GemDragPayload` via `Response::dnd_release_payload`,
            // which lets one zone serve two payload types.
            let mut pending_cross_group_gem_move: Option<(GemDragPayload, usize)> = None;
            let main_socket_group = character.main_socket_group;
            for (idx, group) in character.skill_groups.iter_mut().enumerate() {
                let main_marker = if (idx as u32 + 1) == main_socket_group {
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
                let drop = ui.dnd_drop_zone::<usize, _>(egui::Frame::default(), |ui| {
                    ui.horizontal(|ui| {
                        let drag_id = egui::Id::new(("group_drag", idx));
                        ui.dnd_drag_source(drag_id, idx, |ui| {
                            ui.label(egui::RichText::new("≡").monospace())
                                .on_hover_text("Drag to reorder this group");
                        });
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
                });
                if let Some(payload) = drop.1 {
                    pending_group_move = Some((*payload, idx));
                }
                // Issue #214 (slice 3): the same drop zone also accepts
                // GemDragPayload — route to `move_gem_across_groups`
                // (deferred until after the loop so we can borrow
                // `character` mutably without conflicting with the
                // current iter_mut() iteration).
                if let Some(gem_payload) = drop.0.response.dnd_release_payload::<GemDragPayload>() {
                    pending_cross_group_gem_move = Some((*gem_payload, idx));
                }
            }
            if let Some((payload, to_group)) = pending_cross_group_gem_move {
                if move_gem_across_groups(character, payload.group_idx, payload.gem_idx, to_group) {
                    // Selection follows the dragged gem to its new home so
                    // the right-side detail panel doesn't jump elsewhere.
                    state.selected_group = to_group;
                    state.selected_gem = character.skill_groups[to_group]
                        .gems
                        .len()
                        .saturating_sub(1);
                    changed = true;
                }
            }
            if let Some((from, to)) = pending_group_move {
                if move_skill_group(character, from, to) {
                    // Keep the selection sticky on the dragged group so
                    // the right-side editor doesn't jump to a sibling
                    // after a reorder (same pattern as slice 1's gem
                    // selection handling).
                    state.selected_group = if state.selected_group == from {
                        to
                    } else if from < to && state.selected_group > from && state.selected_group <= to
                    {
                        state.selected_group - 1
                    } else if from > to && state.selected_group >= to && state.selected_group < from
                    {
                        state.selected_group + 1
                    } else {
                        state.selected_group
                    };
                    state.selected_gem = 0;
                    changed = true;
                }
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
            // Issue #214 (slice 1): drag-reorder. Each row is wrapped
            // in a dnd_drop_zone so dropping the dragged gem onto it
            // calls `move_gem(from, idx)`. The "≡" handle on the left
            // is the dnd_drag_source; the rest of the row keeps its
            // existing buttons / hover behaviour.
            let mut pending_move: Option<(usize, usize)> = None;
            let group_id = state.selected_group;
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
                let drop_id = egui::Id::new(("gem_drop", group_id, idx));
                // Issue #214 (slice 3): payload type widened to
                // `GemDragPayload` so cross-group drops on the group-row
                // (handled in the group list above) have the source group
                // index. Same-group drops here keep using `move_gem`; the
                // drop zone ignores payloads from a different group_idx
                // (the user dropped on the wrong target — no-op).
                let drop = ui.dnd_drop_zone::<GemDragPayload, _>(egui::Frame::default(), |ui| {
                    ui.horizontal(|ui| {
                        // Drag handle. Source payload is
                        // `GemDragPayload { group_idx, gem_idx }`; the
                        // drop zone reads `payload.gem_idx` and feeds
                        // `move_gem` for in-group reorder.
                        let drag_id = egui::Id::new(("gem_drag", group_id, idx));
                        ui.dnd_drag_source(
                            drag_id,
                            GemDragPayload {
                                group_idx: group_id,
                                gem_idx: idx,
                            },
                            |ui| {
                                ui.label(egui::RichText::new("≡").monospace())
                                    .on_hover_text("Drag to reorder this gem");
                            },
                        );
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
                        let resp = ui.selectable_label(state.selected_gem == idx, label_text);
                        let resp = if let Some(meta) = skill_meta {
                            // Issue #203 (slice 1): rich gem hover tooltip.
                            let tip_lines = gem_tooltip_lines(meta, gem.level, gem.quality);
                            resp.on_hover_ui(|ui| {
                                for line in &tip_lines {
                                    ui.label(line);
                                }
                            })
                        } else {
                            resp
                        };
                        if resp.clicked() {
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
                });
                let _ = drop_id; // silence dead-code warning if unused
                if let Some(payload) = drop.1 {
                    // Same-group reorder only; cross-group moves are
                    // routed by the group-list drop zone (slice 3).
                    if payload.group_idx == group_id {
                        pending_move = Some((payload.gem_idx, idx));
                    }
                }
            }
            if let Some((from, to)) = pending_move {
                if move_gem(group, from, to) {
                    // Keep the selection sticky on the dragged gem so
                    // the right-side details panel doesn't jump to a
                    // sibling after a reorder.
                    if state.selected_gem == from {
                        state.selected_gem = to;
                    }
                    changed = true;
                }
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

                    // Issue #36: alt-quality (Anomalous / Divergent /
                    // Phantasmal) picker. Always rendered with all
                    // four standard options; a small marker after the
                    // label tells the user which variants have alt
                    // data (the `<base>AltX/Y/Z` skill exists in the
                    // registry). Picking an unsupported variant is
                    // allowed — `skill_for_quality` falls back to the
                    // default qualityStats — but the marker makes it
                    // obvious the dropdown isn't taking effect.
                    let base = base_skill_id(&gem.skill_id).to_owned();
                    let mut chosen_qid = gem.quality_id;
                    egui::ComboBox::from_label("Alt quality")
                        .selected_text(chosen_qid.display())
                        .show_ui(ui, |ui| {
                            for qid in QualityId::all() {
                                let has_data = match qid.alt_suffix() {
                                    None => true,
                                    Some(suffix) => {
                                        registry.get(&format!("{base}{suffix}")).is_some()
                                    }
                                };
                                let label = if has_data {
                                    qid.display().to_owned()
                                } else {
                                    format!("{} (no data)", qid.display())
                                };
                                ui.selectable_value(&mut chosen_qid, qid, label);
                            }
                        });
                    if chosen_qid != gem.quality_id {
                        gem.quality_id = chosen_qid;
                        changed = true;
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
                ui.set_min_width(360.0);
                ui.horizontal(|ui| {
                    ui.heading("Catalog");
                    if ui.button("Close").clicked() {
                        state.catalog_open = false;
                    }
                    // Issue #209 follow-up: blanket reset for the
                    // gem-picker filter facets (search + color chips +
                    // type chips + tag chips). Sticky preferences like
                    // "Hide legacy" stay. Only enables when at least
                    // one facet is active so a cold-open click is
                    // inert.
                    let dirty = gem_picker_filters_active(state);
                    if ui
                        .add_enabled(dirty, egui::Button::new("Reset filters"))
                        .on_hover_text(
                            "Clear the search, color chips, type chips, and tag chips. \
                             Preserves the \"Hide legacy\" and \"Default level on add\" \
                             toggles.",
                        )
                        .clicked()
                    {
                        reset_gem_picker_filters(state);
                    }
                });
                ui.horizontal(|ui| {
                    ui.label("Search:");
                    ui.add(
                        egui::TextEdit::singleline(&mut state.filter)
                            .desired_width(220.0)
                            .hint_text("Arc, fireball, cold, …"),
                    );
                });
                // Color chips. PoB renders these as tinted R/G/B/W buttons; we
                // use checkbox+colored label since egui's selectable_label
                // doesn't tint per-state.
                ui.horizontal(|ui| {
                    ui.label("Color:");
                    let mk = |ui: &mut egui::Ui, on: &mut bool, txt: &str, color: egui::Color32| {
                        let label = egui::RichText::new(txt).color(color).strong();
                        ui.toggle_value(on, label);
                    };
                    mk(
                        ui,
                        &mut state.colors.red,
                        "R",
                        egui::Color32::from_rgb(220, 80, 80),
                    );
                    mk(
                        ui,
                        &mut state.colors.green,
                        "G",
                        egui::Color32::from_rgb(80, 200, 90),
                    );
                    mk(
                        ui,
                        &mut state.colors.blue,
                        "B",
                        egui::Color32::from_rgb(120, 150, 240),
                    );
                    mk(
                        ui,
                        &mut state.colors.white,
                        "W",
                        egui::Color32::from_gray(220),
                    );
                });
                ui.horizontal_wrapped(|ui| {
                    ui.label("Type:");
                    ui.toggle_value(&mut state.types.active, "Active");
                    ui.toggle_value(&mut state.types.support, "Support");
                    ui.toggle_value(&mut state.types.awakened, "Awakened");
                    ui.toggle_value(&mut state.types.exceptional, "Exceptional");
                    ui.toggle_value(&mut state.types.vaal, "Vaal");
                });
                ui.horizontal_wrapped(|ui| {
                    ui.label("Tags:");
                    ui.toggle_value(&mut state.tags.spell, "Spell");
                    ui.toggle_value(&mut state.tags.attack, "Attack");
                    ui.toggle_value(&mut state.tags.aura, "Aura");
                    ui.toggle_value(&mut state.tags.herald, "Herald");
                    ui.toggle_value(&mut state.tags.fire, "Fire");
                    ui.toggle_value(&mut state.tags.cold, "Cold");
                    ui.toggle_value(&mut state.tags.lightning, "Lightning");
                    ui.toggle_value(&mut state.tags.chaos, "Chaos");
                    ui.toggle_value(&mut state.tags.physical, "Physical");
                });
                ui.horizontal(|ui| {
                    ui.checkbox(&mut state.hide_legacy, "Hide legacy")
                        .on_hover_text("Hide removed-from-game gems");
                    ui.checkbox(&mut state.default_level_on_add, "Default level on add")
                        .on_hover_text("Set the gem's level to its natural max level when added");
                });
                let total = registry.len();
                let q = state.filter.trim().to_ascii_lowercase();
                let mut skills: Vec<(&str, &Skill)> = registry
                    .iter()
                    .filter(|(id, s)| passes_filters(id, s, state, &q))
                    .collect();
                skills.sort_by(|a, b| a.1.name.cmp(&b.1.name));
                ui.label(format!("{} of {} skills", skills.len(), total));
                ui.separator();
                egui::ScrollArea::vertical()
                    .id_salt("skill_catalog")
                    .auto_shrink([false, false])
                    .max_height(420.0)
                    .show(ui, |ui| {
                        for (id, s) in skills {
                            let color_marker = skill_color(s);
                            let kind_marker = if s.support { "⚙" } else { " " };
                            let default_lvl = default_gem_level(s);
                            let lvl_hint =
                                default_lvl.map(|l| format!(" L{l}")).unwrap_or_default();
                            let label = format!(
                                "[{}] {} {}{}",
                                color_marker, kind_marker, s.name, lvl_hint
                            );
                            let tokens = skill_tag_tokens(s);
                            let tag_line = if tokens.is_empty() {
                                String::new()
                            } else {
                                tokens.join(" · ")
                            };
                            let resp = ui.selectable_label(false, label);
                            let resp = if tag_line.is_empty() {
                                resp
                            } else {
                                resp.on_hover_text(tag_line)
                            };
                            if resp.clicked() {
                                if let Some(group) =
                                    character.skill_groups.get_mut(state.selected_group)
                                {
                                    let mut gem = MainSkill::new(id);
                                    if state.default_level_on_add {
                                        if let Some(lvl) = default_lvl {
                                            gem.level = lvl;
                                        }
                                    }
                                    group.gems.push(gem);
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

/// Issue #203: build the body of the gem hover tooltip in the Skills
/// tab. Mirrors PoB's `Tooltip:AddLine` calls from `SkillsTab.lua` —
/// the header echoes the row label, then resource cost / cooldown /
/// cast time / damage effectiveness / reservation if any, then the
/// gem's tag chips, finishing with active-vs-support classification.
/// Each entry is one rendered line; an empty string means "spacer".
pub fn gem_tooltip_lines(skill: &Skill, level: u32, quality: u32) -> Vec<String> {
    let mut out = Vec::new();
    out.push(format!(
        "{} (Level {level}, {quality}% quality)",
        skill.name
    ));
    out.push(if skill.support {
        "Support gem".into()
    } else {
        "Active skill gem".into()
    });
    let mana = skill.cost(level, "Mana");
    if mana > 0.0 {
        out.push(format!("Mana cost: {}", mana as i64));
    }
    let life = skill.cost(level, "Life");
    if life > 0.0 {
        out.push(format!("Life cost: {}", life as i64));
    }
    if let Some(cd) = skill.cooldown(level) {
        out.push(format!("Cooldown: {cd:.2}s"));
    }
    let mut tags: Vec<&str> = skill
        .base_flags
        .iter()
        .filter(|(_, v)| **v)
        .map(|(k, _)| k.as_str())
        .collect();
    tags.sort_unstable();
    if !tags.is_empty() {
        out.push(format!("Tags: {}", tags.join(", ")));
    }
    out
}

/// Issue #214 (slice 1): move the gem at `from` to position `to`
/// within `group.gems`, keeping `main_active_skill_index` pointing at
/// the *same* gem (PoB's 1-based index convention is preserved).
/// Returns `true` when the gems vector actually changed.
///
/// Out-of-range indices and a same-index move are no-ops returning
/// `false` — required by issue #214's "drop on invalid targets is a
/// no-op (no panics)" acceptance criterion.
///
/// The main-pointer arithmetic is the subtle part: when the moved
/// gem crosses the main, every gem between the source and target
/// shifts by one index, so the main-pointer's *new* slot depends on
/// its position relative to both endpoints (see the unit tests for
/// each case).
pub(super) fn move_gem(group: &mut SocketGroup, from: usize, to: usize) -> bool {
    let len = group.gems.len();
    if from >= len || to >= len || from == to {
        return false;
    }
    let gem = group.gems.remove(from);
    group.gems.insert(to, gem);
    // PoB stores `main_active_skill_index` as 1-based; `0` (or any
    // value past the end) means "no main set" and we leave it alone.
    if group.main_active_skill_index >= 1 && (group.main_active_skill_index as usize) <= len {
        let main = group.main_active_skill_index as usize - 1;
        let new_main = if main == from {
            to
        } else if from < to && main > from && main <= to {
            main - 1
        } else if from > to && main >= to && main < from {
            main + 1
        } else {
            main
        };
        group.main_active_skill_index = new_main as u32 + 1;
    }
    true
}

/// Issue #214 (slice 2): move the socket group at `from` to position
/// `to` within `character.skill_groups`, keeping `main_socket_group`
/// pointing at the *same* group (PoB's 1-based index convention is
/// preserved — mirrors `move_gem`'s `main_active_skill_index` handling).
/// Returns `true` when the groups vector actually changed.
///
/// Out-of-range indices and a same-index move are no-ops returning
/// `false` — required by issue #214's "drop on invalid targets is a
/// no-op (no panics)" acceptance criterion.
///
/// Caller is responsible for keeping `SkillsTabState::selected_group`
/// in sync (same pattern slice 1 used for `selected_gem`); the helper
/// itself stays state-free so it can be unit-tested in isolation.
pub(super) fn move_skill_group(character: &mut Character, from: usize, to: usize) -> bool {
    let len = character.skill_groups.len();
    if from >= len || to >= len || from == to {
        return false;
    }
    let group = character.skill_groups.remove(from);
    character.skill_groups.insert(to, group);
    // PoB stores `main_socket_group` as 1-based; `0` (or any value past
    // the end) means "no main set" and we leave it alone.
    if character.main_socket_group >= 1 && (character.main_socket_group as usize) <= len {
        let main = character.main_socket_group as usize - 1;
        let new_main = if main == from {
            to
        } else if from < to && main > from && main <= to {
            main - 1
        } else if from > to && main >= to && main < from {
            main + 1
        } else {
            main
        };
        character.main_socket_group = new_main as u32 + 1;
    }
    true
}

/// Issue #214 (slice 3): move the gem at `(from_group, from_idx)` into
/// `to_group`, appending it to the END of the destination group's gems
/// vec. Returns `true` when the move actually happened.
///
/// Same-group moves (`from_group == to_group`) are NOT this helper's
/// job — those go through `move_gem` (slice 1's within-group reorder).
/// Returns `false` in that case so the caller doesn't double-handle.
///
/// Out-of-range indices return `false` without mutation (issue #214 AC
/// #3: drop on invalid targets is a no-op, no panics).
///
/// `main_active_skill_index` handling:
/// - SOURCE group: same arithmetic as `move_gem`'s "remove" half — if
///   the moved gem WAS the main, source main resets to 0 (no main set);
///   if main was AFTER the moved gem, it shifts left by one; otherwise
///   unchanged.
/// - DESTINATION group: untouched. The gem is appended at the end so
///   nothing in front of any existing main pointer shifts.
pub(super) fn move_gem_across_groups(
    character: &mut Character,
    from_group: usize,
    from_idx: usize,
    to_group: usize,
) -> bool {
    if from_group == to_group {
        return false;
    }
    let n_groups = character.skill_groups.len();
    if from_group >= n_groups || to_group >= n_groups {
        return false;
    }
    if from_idx >= character.skill_groups[from_group].gems.len() {
        return false;
    }
    let gem = character.skill_groups[from_group].gems.remove(from_idx);
    // Adjust source group's main pointer (1-based; 0 / past-end means
    // "no main set" and we leave it alone).
    {
        let src = &mut character.skill_groups[from_group];
        // `len_before_remove` = current gems.len() + 1 since we already
        // removed; mirror `move_gem`'s validity check against the pre-
        // removal length.
        let len_before = src.gems.len() + 1;
        if src.main_active_skill_index >= 1 && (src.main_active_skill_index as usize) <= len_before
        {
            let main = src.main_active_skill_index as usize - 1;
            if main == from_idx {
                // Removed gem WAS the main — source group has no main now.
                src.main_active_skill_index = 0;
            } else if main > from_idx {
                src.main_active_skill_index -= 1;
            }
        }
    }
    character.skill_groups[to_group].gems.push(gem);
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use indexmap::IndexMap;
    use pob_engine::character::SocketGroup;
    use serde_json::json;

    fn mk_enabled_group(label: &str, enabled: bool) -> SocketGroup {
        SocketGroup {
            label: label.to_owned(),
            gems: Vec::new(),
            main_active_skill_index: 1,
            enabled,
        }
    }

    #[test]
    fn solo_socket_group_at_disables_others() {
        let mut c = Character::new(pob_engine::ClassRef::ranger(), 1);
        c.skill_groups = vec![
            mk_enabled_group("A", true),
            mk_enabled_group("B", true),
            mk_enabled_group("C", true),
        ];
        assert!(solo_socket_group_at(&mut c, 1));
        let states: Vec<bool> = c.skill_groups.iter().map(|g| g.enabled).collect();
        assert_eq!(states, vec![false, true, false]);
    }

    #[test]
    fn solo_socket_group_at_re_enables_target_when_currently_off() {
        // The target's own `enabled` flag gets flipped true even when
        // it was off — the user may have manually disabled the main
        // group and then clicked Solo.
        let mut c = Character::new(pob_engine::ClassRef::ranger(), 1);
        c.skill_groups = vec![mk_enabled_group("A", false), mk_enabled_group("B", true)];
        assert!(solo_socket_group_at(&mut c, 0));
        let states: Vec<bool> = c.skill_groups.iter().map(|g| g.enabled).collect();
        assert_eq!(states, vec![true, false]);
    }

    #[test]
    fn solo_socket_group_at_returns_false_when_already_solo() {
        // Already at the target state → no change, the caller can
        // skip the recompute flip.
        let mut c = Character::new(pob_engine::ClassRef::ranger(), 1);
        c.skill_groups = vec![mk_enabled_group("A", true), mk_enabled_group("B", false)];
        assert!(!solo_socket_group_at(&mut c, 0));
    }

    #[test]
    fn solo_socket_group_at_out_of_range_is_noop() {
        let mut c = Character::new(pob_engine::ClassRef::ranger(), 1);
        c.skill_groups = vec![mk_enabled_group("A", true)];
        assert!(!solo_socket_group_at(&mut c, 7));
        // Original group's flag is untouched.
        assert!(c.skill_groups[0].enabled);
    }

    #[test]
    fn enable_all_socket_groups_flips_disabled_back_on() {
        let mut c = Character::new(pob_engine::ClassRef::ranger(), 1);
        c.skill_groups = vec![
            mk_enabled_group("A", false),
            mk_enabled_group("B", true),
            mk_enabled_group("C", false),
        ];
        assert!(enable_all_socket_groups(&mut c));
        for g in &c.skill_groups {
            assert!(g.enabled, "group {} should be enabled", g.label);
        }
    }

    #[test]
    fn enable_all_socket_groups_returns_false_when_already_all_on() {
        // No-op signal so the caller can skip a recompute.
        let mut c = Character::new(pob_engine::ClassRef::ranger(), 1);
        c.skill_groups = vec![mk_enabled_group("A", true), mk_enabled_group("B", true)];
        assert!(!enable_all_socket_groups(&mut c));
    }

    #[test]
    fn disable_all_socket_groups_flips_enabled_off() {
        // Mirror of the enable_all test: every truthy flag drops to
        // false; an already-off group is untouched.
        let mut c = Character::new(pob_engine::ClassRef::ranger(), 1);
        c.skill_groups = vec![
            mk_enabled_group("A", true),
            mk_enabled_group("B", false),
            mk_enabled_group("C", true),
        ];
        assert!(disable_all_socket_groups(&mut c));
        for g in &c.skill_groups {
            assert!(!g.enabled, "group {} should be disabled", g.label);
        }
    }

    #[test]
    fn disable_all_socket_groups_returns_false_when_already_all_off() {
        // No-op signal so a Disable-all click on an already-off roster
        // doesn't dirty the build.
        let mut c = Character::new(pob_engine::ClassRef::ranger(), 1);
        c.skill_groups = vec![mk_enabled_group("A", false), mk_enabled_group("B", false)];
        assert!(!disable_all_socket_groups(&mut c));
    }

    #[test]
    fn count_active_socket_groups_returns_zero_zero_for_empty_roster() {
        // The chip is suppressed when the roster is empty (the wrapping
        // `if !character.skill_groups.is_empty()` guards the whole bar),
        // but the helper itself must handle the case without surprises.
        let c = Character::new(pob_engine::ClassRef::ranger(), 1);
        assert_eq!(count_active_socket_groups(&c), (0, 0));
    }

    #[test]
    fn count_active_socket_groups_returns_active_and_total_separately() {
        // Mixed roster: chip renders "1 of 3 on" so the user spots
        // a soloed group at a glance.
        let mut c = Character::new(pob_engine::ClassRef::ranger(), 1);
        c.skill_groups = vec![
            mk_enabled_group("A", true),
            mk_enabled_group("B", false),
            mk_enabled_group("C", false),
        ];
        assert_eq!(count_active_socket_groups(&c), (1, 3));
    }

    fn mk_skill(name: &str, color: u8, flags: &[&str], stats: &[&str], support: bool) -> Skill {
        let mut base_flags: IndexMap<String, bool> = IndexMap::new();
        for f in flags {
            base_flags.insert((*f).to_owned(), true);
        }
        Skill {
            name: name.to_owned(),
            base_type_name: name.to_owned(),
            color,
            description: String::new(),
            cast_time: 0.0,
            base_flags,
            quality_stats: Vec::new(),
            constant_stats: Vec::new(),
            stats: stats.iter().map(|s| (*s).to_owned()).collect(),
            not_minion_stat: Vec::new(),
            skill_types: IndexMap::new(),
            // 20 dummy levels so default_gem_level returns 20.
            levels: (0..20).map(|_| json!({})).collect(),
            stat_map: IndexMap::new(),
            support,
            add_skill_types: IndexMap::new(),
            exclude_skill_types: IndexMap::new(),
            base_effectiveness: 1.0,
            incremental_effectiveness: 0.0,
            minion_list: Vec::new(),
        }
    }

    #[test]
    fn color_filter_narrows_picker() {
        let arc = mk_skill(
            "Arc",
            3,
            &["spell"],
            &["spell_lightning_base_damage"],
            false,
        );
        let cleave = mk_skill("Cleave", 1, &["attack"], &["physical_attack_damage"], false);
        let mut state = SkillsTabState {
            colors: ColorFilter {
                blue: true,
                ..Default::default()
            },
            ..Default::default()
        };
        assert!(passes_filters("Arc", &arc, &state, ""));
        assert!(!passes_filters("Cleave", &cleave, &state, ""));
        // With only red ticked, the spell drops out.
        state.colors = ColorFilter {
            red: true,
            ..Default::default()
        };
        assert!(!passes_filters("Arc", &arc, &state, ""));
        assert!(passes_filters("Cleave", &cleave, &state, ""));
    }

    #[test]
    fn tag_chip_and_search_match_tag_tokens() {
        let arc = mk_skill(
            "Arc",
            3,
            &["spell"],
            &["spell_lightning_base_damage"],
            false,
        );
        let fireball = mk_skill(
            "Fireball",
            3,
            &["spell"],
            &["spell_fire_base_damage"],
            false,
        );
        // Tag chip narrows.
        let mut state = SkillsTabState {
            tags: TagFilter {
                lightning: true,
                ..Default::default()
            },
            ..Default::default()
        };
        assert!(passes_filters("Arc", &arc, &state, ""));
        assert!(!passes_filters("Fireball", &fireball, &state, ""));
        // Search query against tag token (Lightning) matches even though the
        // skill name is "Arc".
        state.tags = TagFilter::default();
        assert!(passes_filters("Arc", &arc, &state, "lightning"));
        assert!(!passes_filters("Fireball", &fireball, &state, "lightning"));
    }

    // ─── reset_gem_picker_filters / gem_picker_filters_active ────────────

    #[test]
    fn gem_picker_filters_active_default_state_is_inactive() {
        // Cold-open: every chip off, search empty → Reset button is
        // disabled.
        let state = SkillsTabState::default();
        assert!(!gem_picker_filters_active(&state));
    }

    #[test]
    fn gem_picker_filters_active_each_facet_disqualifies() {
        // Each facet should flip the bit independently — guards
        // against an accidental short-circuit in a future refactor.
        let mut s = SkillsTabState {
            filter: "x".into(),
            ..Default::default()
        };
        assert!(gem_picker_filters_active(&s));

        s = SkillsTabState::default();
        s.colors.red = true;
        assert!(gem_picker_filters_active(&s));

        s = SkillsTabState::default();
        s.tags.fire = true;
        assert!(gem_picker_filters_active(&s));

        s = SkillsTabState::default();
        s.types.support = true;
        assert!(gem_picker_filters_active(&s));
    }

    #[test]
    fn gem_picker_filters_active_whitespace_search_counts_as_inactive() {
        // Trim semantics match the Items-tab BrowseFilter rule — a
        // whitespace-only buffer shouldn't keep the Reset button lit.
        let s = SkillsTabState {
            filter: "   ".into(),
            ..Default::default()
        };
        assert!(!gem_picker_filters_active(&s));
    }

    #[test]
    fn reset_gem_picker_filters_clears_every_facet() {
        let mut s = SkillsTabState {
            filter: "Arc".into(),
            ..Default::default()
        };
        s.colors.red = true;
        s.colors.blue = true;
        s.tags.lightning = true;
        s.types.active = true;
        let changed = reset_gem_picker_filters(&mut s);
        assert!(changed);
        assert!(s.filter.is_empty());
        assert!(!s.colors.any());
        assert!(!s.tags.any());
        assert!(!s.types.any());
    }

    #[test]
    fn reset_gem_picker_filters_preserves_sticky_preferences() {
        // `hide_legacy` and `default_level_on_add` are user prefs, not
        // transient view state — they survive the reset.
        let mut s = SkillsTabState {
            filter: "x".into(),
            hide_legacy: false,
            default_level_on_add: false,
            ..Default::default()
        };
        let _ = reset_gem_picker_filters(&mut s);
        assert!(!s.hide_legacy, "hide_legacy must survive the reset");
        assert!(
            !s.default_level_on_add,
            "default_level_on_add must survive the reset"
        );
    }

    #[test]
    fn reset_gem_picker_filters_no_op_returns_false() {
        // No-op signal so a Reset click on a cold-open catalog
        // doesn't dirty downstream state.
        let mut s = SkillsTabState::default();
        assert!(!reset_gem_picker_filters(&mut s));
    }

    #[test]
    fn hide_legacy_drops_removed_skills() {
        let removed = mk_skill(
            "Removed Stuff",
            3,
            &["spell"],
            &["spell_fire_base_damage"],
            false,
        );
        let mut state = SkillsTabState {
            hide_legacy: true,
            ..Default::default()
        };
        assert!(!passes_filters("Removed_Skill", &removed, &state, ""));
        state.hide_legacy = false;
        assert!(passes_filters("Removed_Skill", &removed, &state, ""));
    }

    #[test]
    fn gem_tooltip_first_line_shows_name_level_quality() {
        // Issue #203: rich gem tooltips. The header line is what the
        // user reads first — it should mirror the row label so the
        // hover doesn't feel disjointed from the click target.
        let arc = mk_skill(
            "Arc",
            3,
            &["spell"],
            &["spell_lightning_base_damage"],
            false,
        );
        let lines = gem_tooltip_lines(&arc, 20, 23);
        assert!(
            lines.first().map(String::as_str) == Some("Arc (Level 20, 23% quality)"),
            "first line was {:?}",
            lines.first()
        );
    }

    #[test]
    fn gem_tooltip_classifies_active_vs_support() {
        // Knowing whether you're hovering a support or active gem is
        // the most actionable bit — supports without an active above
        // them in the link group do nothing, and the user fishing a
        // gem out of the catalog tooltip needs to see this immediately.
        let active = mk_skill("Arc", 3, &["spell"], &[], false);
        let support = mk_skill("Added Cold Damage", 2, &[], &[], true);
        let active_lines = gem_tooltip_lines(&active, 1, 0);
        let support_lines = gem_tooltip_lines(&support, 1, 0);
        assert!(
            active_lines.iter().any(|l| l == "Active skill gem"),
            "missing active marker: {active_lines:?}"
        );
        assert!(
            support_lines.iter().any(|l| l == "Support gem"),
            "missing support marker: {support_lines:?}"
        );
    }

    #[test]
    fn gem_tooltip_surfaces_mana_cost_when_present() {
        // Mana cost is read off the level entry's `cost.Mana` field.
        // We render it whole (no decimals) since PoB writes integer
        // costs in the data files for active spells.
        let mut arc = mk_skill("Arc", 3, &["spell"], &[], false);
        arc.levels = vec![json!({"cost": {"Mana": 16}})];
        let lines = gem_tooltip_lines(&arc, 1, 0);
        assert!(
            lines.iter().any(|l| l == "Mana cost: 16"),
            "missing mana cost line: {lines:?}"
        );
    }

    #[test]
    fn gem_tooltip_omits_cost_lines_when_zero() {
        // Cost = 0 means free (e.g. some auras report no mana cost,
        // or a cosmetic gem). Showing "Mana cost: 0" would be noise.
        let arc = mk_skill("Arc", 3, &["spell"], &[], false);
        let lines = gem_tooltip_lines(&arc, 1, 0);
        assert!(
            !lines.iter().any(|l| l.starts_with("Mana cost")),
            "spurious mana cost line: {lines:?}"
        );
        assert!(
            !lines.iter().any(|l| l.starts_with("Life cost")),
            "spurious life cost line: {lines:?}"
        );
    }

    #[test]
    fn gem_tooltip_surfaces_cooldown_when_present_omits_when_absent() {
        // Most attacks / instant spells lack an explicit cooldown.
        // We render a "Cooldown: <s>s" line only when the level entry
        // carries one (`Skill::cooldown` returns `Some`).
        let mut warlords = mk_skill("Warlord's Mark", 3, &["spell"], &[], false);
        warlords.levels = vec![json!({"cooldown": 1.5})];
        let lines = gem_tooltip_lines(&warlords, 1, 0);
        assert!(
            lines.iter().any(|l| l == "Cooldown: 1.50s"),
            "missing cooldown line: {lines:?}"
        );

        let mut arc = mk_skill("Arc", 3, &["spell"], &[], false);
        arc.levels = vec![json!({})];
        let lines = gem_tooltip_lines(&arc, 1, 0);
        assert!(
            !lines.iter().any(|l| l.starts_with("Cooldown")),
            "spurious cooldown line: {lines:?}"
        );
    }

    #[test]
    fn gem_tooltip_lists_active_base_flags_as_tags() {
        // Tags drive support-gem applicability (a Spell support only
        // sticks to a gem with the "spell" base flag) and the user
        // needs to see them at a glance. PoB renders these as a
        // comma-joined tag chip row.
        let arc = mk_skill("Arc", 3, &["spell", "lightning", "chaining"], &[], false);
        let lines = gem_tooltip_lines(&arc, 1, 0);
        let tag_line = lines
            .iter()
            .find(|l| l.starts_with("Tags: "))
            .unwrap_or_else(|| panic!("missing tags line: {lines:?}"));
        // Tag order is alphabetical (deterministic regardless of
        // base_flags insertion order).
        assert_eq!(tag_line, "Tags: chaining, lightning, spell");
    }

    fn mk_socket_group(skill_ids: &[&str], main_one_based: u32) -> SocketGroup {
        SocketGroup {
            label: "Test".into(),
            gems: skill_ids
                .iter()
                .map(|id| MainSkill {
                    skill_id: (*id).into(),
                    level: 20,
                    quality: 0,
                    quality_id: QualityId::Default,
                    enabled: true,
                })
                .collect(),
            main_active_skill_index: main_one_based,
            enabled: true,
        }
    }

    #[test]
    fn move_gem_forward_shifts_intermediate_main_back_one() {
        // Issue #214 (slice 1): drag-reorder helper. Moving gem at
        // idx 0 forward to idx 2 in [A, B, C, D] yields [B, C, A, D].
        // The main pointer (1-based, started at 2 = "B") must keep
        // tracking B — its new home is idx 0, so the index drops to 1.
        let mut g = mk_socket_group(&["A", "B", "C", "D"], 2);
        assert!(move_gem(&mut g, 0, 2));
        let ids: Vec<&str> = g.gems.iter().map(|g| g.skill_id.as_str()).collect();
        assert_eq!(ids, vec!["B", "C", "A", "D"]);
        assert_eq!(g.main_active_skill_index, 1, "main should still be B");
    }

    #[test]
    fn move_gem_backward_shifts_intermediate_main_forward_one() {
        // Reverse direction: idx 3 → idx 0 in [A, B, C, D] yields
        // [D, A, B, C]. Main pointed at C (idx 2, 1-based 3) which
        // shifts down by 1 idx → new 1-based index 4.
        let mut g = mk_socket_group(&["A", "B", "C", "D"], 3);
        assert!(move_gem(&mut g, 3, 0));
        let ids: Vec<&str> = g.gems.iter().map(|g| g.skill_id.as_str()).collect();
        assert_eq!(ids, vec!["D", "A", "B", "C"]);
        assert_eq!(g.main_active_skill_index, 4, "main should still be C");
    }

    #[test]
    fn move_gem_moving_main_itself_follows_to_new_index() {
        // Drag the *main* gem. Result: main pointer follows to its
        // new position. [A, B, C], main=2 (B). Move 1→2 ⇒ [A, C, B],
        // main now 3 (B is at idx 2).
        let mut g = mk_socket_group(&["A", "B", "C"], 2);
        assert!(move_gem(&mut g, 1, 2));
        let ids: Vec<&str> = g.gems.iter().map(|g| g.skill_id.as_str()).collect();
        assert_eq!(ids, vec!["A", "C", "B"]);
        assert_eq!(g.main_active_skill_index, 3);
    }

    #[test]
    fn move_gem_unrelated_position_leaves_main_unchanged() {
        // Move idx 2 → idx 3 in [A, B, C, D, E] with main = 1 (A).
        // A doesn't move and isn't crossed by the swap, so main stays.
        let mut g = mk_socket_group(&["A", "B", "C", "D", "E"], 1);
        assert!(move_gem(&mut g, 2, 3));
        let ids: Vec<&str> = g.gems.iter().map(|g| g.skill_id.as_str()).collect();
        assert_eq!(ids, vec!["A", "B", "D", "C", "E"]);
        assert_eq!(g.main_active_skill_index, 1);
    }

    #[test]
    fn move_gem_no_op_returns_false() {
        // Drop on invalid targets is a no-op (issue #214 AC #3): out
        // of range → false, same-index → false, both with no mutation.
        let original_ids = ["A", "B"];
        let mut g = mk_socket_group(&original_ids, 1);
        assert!(!move_gem(&mut g, 0, 0));
        assert!(!move_gem(&mut g, 99, 0));
        assert!(!move_gem(&mut g, 0, 99));
        let ids: Vec<&str> = g.gems.iter().map(|g| g.skill_id.as_str()).collect();
        assert_eq!(ids, original_ids.to_vec());
        assert_eq!(g.main_active_skill_index, 1);
    }

    fn mk_character_with_groups(labels: &[&str], main_one_based: u32) -> Character {
        use pob_engine::character::ClassRef;
        let mut c = Character::new(ClassRef::marauder(), 1);
        c.skill_groups = labels
            .iter()
            .map(|l| SocketGroup {
                label: (*l).into(),
                gems: Vec::new(),
                main_active_skill_index: 1,
                enabled: true,
            })
            .collect();
        c.main_socket_group = main_one_based;
        c
    }

    #[test]
    fn move_skill_group_forward_shifts_intermediate_main_back_one() {
        // Issue #214 (slice 2): drag-reorder socket groups. Moving group
        // at idx 0 forward to idx 2 in [A, B, C, D] yields [B, C, A, D].
        // `main_socket_group` was pointing at B (1-based 2); B is now at
        // idx 0 so the 1-based pointer drops to 1 — same shift arithmetic
        // as `move_gem` applies to `main_active_skill_index`.
        let mut c = mk_character_with_groups(&["A", "B", "C", "D"], 2);
        assert!(move_skill_group(&mut c, 0, 2));
        let labels: Vec<&str> = c.skill_groups.iter().map(|g| g.label.as_str()).collect();
        assert_eq!(labels, vec!["B", "C", "A", "D"]);
        assert_eq!(
            c.main_socket_group, 1,
            "main_socket_group should still be B"
        );
    }

    #[test]
    fn move_skill_group_backward_shifts_intermediate_main_forward_one() {
        // Reverse: idx 3 → idx 0 in [A, B, C, D] yields [D, A, B, C].
        // main_socket_group was C (idx 2, 1-based 3) → shifts up by 1 → 4.
        let mut c = mk_character_with_groups(&["A", "B", "C", "D"], 3);
        assert!(move_skill_group(&mut c, 3, 0));
        let labels: Vec<&str> = c.skill_groups.iter().map(|g| g.label.as_str()).collect();
        assert_eq!(labels, vec!["D", "A", "B", "C"]);
        assert_eq!(
            c.main_socket_group, 4,
            "main_socket_group should still be C"
        );
    }

    #[test]
    fn move_skill_group_moving_main_itself_follows_to_new_index() {
        // Drag the *main* group. [A, B, C], main = 2 (B). Move 1→2 ⇒
        // [A, C, B], main now 3 (B is at idx 2).
        let mut c = mk_character_with_groups(&["A", "B", "C"], 2);
        assert!(move_skill_group(&mut c, 1, 2));
        let labels: Vec<&str> = c.skill_groups.iter().map(|g| g.label.as_str()).collect();
        assert_eq!(labels, vec!["A", "C", "B"]);
        assert_eq!(c.main_socket_group, 3);
    }

    #[test]
    fn move_skill_group_unrelated_position_leaves_main_unchanged() {
        // Move idx 2 → idx 3 in [A, B, C, D, E] with main = 1 (A). The
        // main group doesn't move and isn't crossed by the swap, so main
        // stays at 1.
        let mut c = mk_character_with_groups(&["A", "B", "C", "D", "E"], 1);
        assert!(move_skill_group(&mut c, 2, 3));
        let labels: Vec<&str> = c.skill_groups.iter().map(|g| g.label.as_str()).collect();
        assert_eq!(labels, vec!["A", "B", "D", "C", "E"]);
        assert_eq!(c.main_socket_group, 1);
    }

    #[test]
    fn move_skill_group_no_op_returns_false() {
        // Issue #214 AC #3: drop on invalid targets is a no-op. Out of
        // range → false, same-index → false; no mutation in either case.
        let mut c = mk_character_with_groups(&["A", "B"], 1);
        assert!(!move_skill_group(&mut c, 0, 0));
        assert!(!move_skill_group(&mut c, 99, 0));
        assert!(!move_skill_group(&mut c, 0, 99));
        let labels: Vec<&str> = c.skill_groups.iter().map(|g| g.label.as_str()).collect();
        assert_eq!(labels, vec!["A", "B"]);
        assert_eq!(c.main_socket_group, 1);
    }

    fn mk_character_with_gemmed_groups(
        groups: &[(&[&str], u32)],
        main_one_based: u32,
    ) -> Character {
        use pob_engine::character::ClassRef;
        let mut c = Character::new(ClassRef::marauder(), 1);
        c.skill_groups = groups
            .iter()
            .enumerate()
            .map(|(i, (ids, main))| SocketGroup {
                label: format!("G{}", i + 1),
                gems: ids
                    .iter()
                    .map(|id| MainSkill {
                        skill_id: (*id).into(),
                        level: 20,
                        quality: 0,
                        quality_id: QualityId::Default,
                        enabled: true,
                    })
                    .collect(),
                main_active_skill_index: *main,
                enabled: true,
            })
            .collect();
        c.main_socket_group = main_one_based;
        c
    }

    #[test]
    fn move_gem_across_groups_forward_appends_and_clears_source_main() {
        // Issue #214 (slice 3): drag a gem from group A onto group B's
        // header. Source had main = the dragged gem ⇒ source main resets
        // to 0 (no main). Destination main is unchanged (gem appended at
        // end, nothing in front shifts).
        let mut c =
            mk_character_with_gemmed_groups(&[(&["A", "B", "C"][..], 1), (&["X", "Y"][..], 2)], 1);
        assert!(move_gem_across_groups(&mut c, 0, 0, 1));
        let g0_ids: Vec<&str> = c.skill_groups[0]
            .gems
            .iter()
            .map(|g| g.skill_id.as_str())
            .collect();
        let g1_ids: Vec<&str> = c.skill_groups[1]
            .gems
            .iter()
            .map(|g| g.skill_id.as_str())
            .collect();
        assert_eq!(g0_ids, vec!["B", "C"]);
        assert_eq!(g1_ids, vec!["X", "Y", "A"]);
        assert_eq!(
            c.skill_groups[0].main_active_skill_index, 0,
            "moved gem WAS the main of the source group, so source main resets to 0"
        );
        assert_eq!(
            c.skill_groups[1].main_active_skill_index, 2,
            "destination main untouched (gem appended at end)"
        );
    }

    #[test]
    fn move_gem_across_groups_source_main_after_moved_gem_decrements() {
        // Source main pointed at C (1-based 3); we remove A (idx 0).
        // Everything after A shifts left by one ⇒ source main becomes 2.
        let mut c =
            mk_character_with_gemmed_groups(&[(&["A", "B", "C"][..], 3), (&["X"][..], 1)], 1);
        assert!(move_gem_across_groups(&mut c, 0, 0, 1));
        assert_eq!(
            c.skill_groups[0].main_active_skill_index, 2,
            "source main shifted from 3 → 2 because gem before it was removed"
        );
        assert_eq!(c.skill_groups[1].main_active_skill_index, 1);
    }

    #[test]
    fn move_gem_across_groups_backward_works_too() {
        // Move from group 1 (later) into group 0 (earlier). Destination
        // is "earlier" but the from_group/to_group ordering doesn't matter
        // for the helper — just exercise the reverse direction.
        let mut c =
            mk_character_with_gemmed_groups(&[(&["A", "B"][..], 1), (&["X", "Y", "Z"][..], 2)], 2);
        assert!(move_gem_across_groups(&mut c, 1, 0, 0));
        let g0_ids: Vec<&str> = c.skill_groups[0]
            .gems
            .iter()
            .map(|g| g.skill_id.as_str())
            .collect();
        let g1_ids: Vec<&str> = c.skill_groups[1]
            .gems
            .iter()
            .map(|g| g.skill_id.as_str())
            .collect();
        assert_eq!(g0_ids, vec!["A", "B", "X"]);
        assert_eq!(g1_ids, vec!["Y", "Z"]);
        // Source group: main was idx 1 (Y), removing idx 0 (X) ⇒ main shifts
        // 2 → 1.
        assert_eq!(c.skill_groups[1].main_active_skill_index, 1);
        // Destination group main untouched.
        assert_eq!(c.skill_groups[0].main_active_skill_index, 1);
    }

    #[test]
    fn move_gem_across_groups_source_becomes_empty() {
        // Last gem in source moved away. Source ends up empty; main was
        // pointing at the moved gem ⇒ resets to 0 (no main).
        let mut c = mk_character_with_gemmed_groups(&[(&["A"][..], 1), (&[][..], 0)], 1);
        assert!(move_gem_across_groups(&mut c, 0, 0, 1));
        assert!(c.skill_groups[0].gems.is_empty());
        assert_eq!(c.skill_groups[0].main_active_skill_index, 0);
        let g1_ids: Vec<&str> = c.skill_groups[1]
            .gems
            .iter()
            .map(|g| g.skill_id.as_str())
            .collect();
        assert_eq!(g1_ids, vec!["A"]);
    }

    #[test]
    fn move_gem_across_groups_same_group_is_noop() {
        // from_group == to_group is slice 1's job; helper returns false
        // and doesn't mutate.
        let mut c = mk_character_with_gemmed_groups(&[(&["A", "B"][..], 1)], 1);
        assert!(!move_gem_across_groups(&mut c, 0, 0, 0));
        let ids: Vec<&str> = c.skill_groups[0]
            .gems
            .iter()
            .map(|g| g.skill_id.as_str())
            .collect();
        assert_eq!(ids, vec!["A", "B"]);
        assert_eq!(c.skill_groups[0].main_active_skill_index, 1);
    }

    #[test]
    fn move_gem_across_groups_out_of_range_returns_false() {
        // Issue #214 AC #3: invalid drops are no-ops, no panics.
        let mut c = mk_character_with_gemmed_groups(&[(&["A"][..], 1), (&["X"][..], 1)], 1);
        // Bad from_group.
        assert!(!move_gem_across_groups(&mut c, 99, 0, 1));
        // Bad to_group.
        assert!(!move_gem_across_groups(&mut c, 0, 0, 99));
        // Bad gem index.
        assert!(!move_gem_across_groups(&mut c, 0, 99, 1));
        // Nothing changed.
        let g0: Vec<&str> = c.skill_groups[0]
            .gems
            .iter()
            .map(|g| g.skill_id.as_str())
            .collect();
        let g1: Vec<&str> = c.skill_groups[1]
            .gems
            .iter()
            .map(|g| g.skill_id.as_str())
            .collect();
        assert_eq!(g0, vec!["A"]);
        assert_eq!(g1, vec!["X"]);
    }

    #[test]
    fn default_gem_level_uses_levels_len_capped_at_20() {
        let mut s = mk_skill(
            "Arc",
            3,
            &["spell"],
            &["spell_lightning_base_damage"],
            false,
        );
        // 20 levels in fixture → default 20.
        assert_eq!(default_gem_level(&s), Some(20));
        // Pad to 25 (awakened-style); cap should still report 20.
        s.levels = (0..25).map(|_| json!({})).collect();
        assert_eq!(default_gem_level(&s), Some(20));
        // No data → None.
        s.levels.clear();
        assert_eq!(default_gem_level(&s), None);
    }
}
