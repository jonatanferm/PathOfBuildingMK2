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

#[cfg(test)]
mod tests {
    use super::*;
    use indexmap::IndexMap;
    use serde_json::json;

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
