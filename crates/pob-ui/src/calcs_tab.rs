//! Calcs tab — flat dump of every computed output stat plus a click-to-drill-down
//! panel that walks the contributing modifiers from the live ModDB.
//!
//! Slice 2 of [#34](https://github.com/jonatanferm/PathOfBuildingMK2/issues/34)
//! adds an opt-in "PoB layout" view that renders the imported
//! [`pob_data::CalcSection`] tree in PoB's three-column group layout. The legacy
//! flat-key view remains the default until enough breakdown rows have been ported
//! to make the section view pull its weight.

use std::collections::HashSet;

use eframe::egui;
use pob_data::{CalcRow, CalcSection};
use pob_engine::{
    derive_for, Breakdown, BreakdownStep, Character, Env, Mod, ModSource, ModStore as _, ModType,
    Output, SkillRegistry, Source, Tag,
};

#[derive(Default)]
pub struct CalcsTabState {
    pub filter: String,
    pub hide_zero: bool,
    /// Stat the user clicked to inspect. `None` collapses the breakdown panel.
    pub focused_stat: Option<String>,
    /// When `true` and `calc_sections` is loaded, render the PoB-style grouped
    /// section layout instead of the flat key list.
    pub use_pob_layout: bool,
}

/// Stat category groupings — each (heading, prefix-or-substring-list).
/// Section order + names track upstream PoB's
/// `Modules/CalcSections.lua` structure: Offence groups first
/// (Skill Hit Damage / Speed / Crit / Accuracy / Impale / Bleed /
/// Poison / Ignite / Other Effects), then Attributes, then Defence
/// (Resists / Damage Avoidance / Charges / Other Defences). A full
/// CalcSections.lua port (#34) keeps section-row breakdowns scoped
/// to each stat, but matching the layout already gets us most of the
/// usability win.
const GROUPS: &[(&str, &[&str])] = &[
    // Issue #19: warcry / exertion outputs. Listed first so keys
    // like `ExertedAttackDamageBonus` don't get swept into the
    // generic "Skill Hit Damage" group by the substring match.
    // Covers slice-3 loadout aggregates (`ActiveWarcryCount`,
    // `WarcryExertedAttackCountTotal`, `WarcryMinCooldown`), the
    // slice-2 Config knob (`WarcryPower`), the slice-4 auto-uptime
    // (`ExertedAttackUptime`, `ExertedAttackDamageBonus`), and the
    // slice-6 Intimidating-Cry indicator (`IntimidatingCryActive`).
    ("Warcry", &["Warcry", "Exerted", "Cry"]),
    // Issue #84: mine / trap timing outputs. Listed before
    // "Attack / Cast Rate" so `MineLayingSpeed` /
    // `TrapThrowingSpeed` aren't absorbed by the generic "Speed"
    // pattern.
    ("Mines / Traps", &["Mine", "Trap"]),
    // Issue #20 (slices 3-6): minion outputs. Listed before
    // "Skill Hit Damage" / "Pools" / "Resists" so keys like
    // `MinionLife` / `MinionFireResist` / `MinionDPS` don't get
    // absorbed by the generic `Life` / `FireResist` / `Damage`
    // patterns. The single-prefix `Minion` substring catches every
    // key the engine emits today (`MinionLife*`, `MinionDamage*`,
    // `MinionAttacksPerSecond*`, `MinionCritChance` /
    // `MinionCritMultiplier`, `Minion{Fire,Cold,Lightning,Chaos}Resist*`,
    // `MinionDPS`).
    ("Minion", &["Minion"]),
    // OFFENCE column.
    (
        "Skill Hit Damage",
        &[
            "MainSkill",
            "FullDPS",
            "WithBleedDPS",
            "WithImpaleDPS",
            "Damage",
        ],
    ),
    (
        "Attack / Cast Rate",
        &["Speed", "AttackSpeed", "CastSpeed", "MainSkillSpeed"],
    ),
    ("Crits", &["Crit", "CritChance", "CritMultiplier"]),
    ("Impale", &["Impale"]),
    ("Accuracy", &["Accuracy", "HitChance"]),
    ("Bleed", &["Bleed"]),
    ("Poison", &["Poison"]),
    ("Ignite", &["Ignite"]),
    (
        "Non-Damaging Ailments",
        &["Freeze", "Shock", "Chill", "Scorch", "Ailment"],
    ),
    ("Other Offence", &["Projectile", "Chain", "AoE", "Area"]),
    // CORE / NORMAL.
    (
        "Attributes",
        &["Strength", "Dexterity", "Intelligence", "AllAttributes"],
    ),
    ("Pools", &["Life", "Mana", "EnergyShield", "Ward", "Rage"]),
    // DEFENCE column.
    (
        "Resists",
        &[
            "FireResist",
            "ColdResist",
            "LightningResist",
            "ChaosResist",
            "ElementalResist",
        ],
    ),
    ("Damage Avoidance", &["Block", "Suppress", "Dodge", "Avoid"]),
    (
        "Charges",
        &["Charge", "PowerCharge", "FrenzyCharge", "EnduranceCharge"],
    ),
    (
        "Other Defences",
        &[
            "Armour", "Evasion", "Recover", "Regen", "Recharge", "Phys", "EHP",
        ],
    ),
    ("Misc", &["Misc:", "Keystone:"]),
];

pub fn ui(
    ui: &mut egui::Ui,
    state: &mut CalcsTabState,
    output: &Output,
    env: Option<&Env>,
    calc_sections: Option<&[CalcSection]>,
    active_skill_flags: &HashSet<String>,
) {
    ui.horizontal(|ui| {
        ui.label("Filter:");
        ui.add(
            egui::TextEdit::singleline(&mut state.filter)
                .desired_width(220.0)
                .hint_text("Life, FireResist, MainSkill, …"),
        );
        ui.checkbox(&mut state.hide_zero, "Hide zero values");
        if calc_sections.is_some() {
            ui.checkbox(&mut state.use_pob_layout, "PoB layout")
                .on_hover_text(
                    "Render the Calcs tab in PoB's three-column section layout, sourced from \
                     Modules/CalcSections.lua. Falls back to the flat key list when off.",
                );
        }
        ui.separator();
        ui.label(format!("{} stats", output.len()));
        if state.focused_stat.is_some() {
            if ui.button("Close breakdown").clicked() {
                state.focused_stat = None;
            }
        }
    });
    ui.separator();

    if state.use_pob_layout {
        if let Some(sections) = calc_sections {
            render_pob_layout(ui, state, sections, output, env, active_skill_flags);
            return;
        }
    }

    let q = state.filter.trim().to_lowercase();
    let mut entries: Vec<(&str, f64)> = output.iter().collect();
    entries.sort_by(|a, b| a.0.cmp(b.0));
    let entries_filtered: Vec<(&str, f64)> = entries
        .into_iter()
        .filter(|(k, _)| q.is_empty() || k.to_lowercase().contains(&q))
        .filter(|(_, v)| !state.hide_zero || v.abs() > 1e-9)
        .collect();

    ui.horizontal(|ui| {
        // Left pane: stat list.
        ui.vertical(|ui| {
            let breakdown_open = state.focused_stat.is_some();
            let target_width = if breakdown_open { 380.0 } else { f32::INFINITY };
            ui.set_min_width(360.0);
            if breakdown_open {
                ui.set_max_width(target_width);
            }
            egui::ScrollArea::vertical()
                .id_salt("calcs_list")
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    let mut shown: std::collections::HashSet<&str> = Default::default();
                    for (heading, patterns) in GROUPS {
                        let group_entries: Vec<&(&str, f64)> = entries_filtered
                            .iter()
                            .filter(|(k, _)| {
                                patterns.iter().any(|p| {
                                    if p.ends_with(':') {
                                        k.starts_with(p)
                                    } else {
                                        k.contains(p)
                                    }
                                })
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
                                        render_row(ui, k, *v, &mut state.focused_stat);
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
                                        render_row(ui, k, *v, &mut state.focused_stat);
                                    }
                                });
                        });
                    }
                });
        });

        // Right pane: breakdown for the focused stat.
        if let Some(focus) = state.focused_stat.clone() {
            ui.separator();
            ui.vertical(|ui| {
                render_focused_breakdown(ui, env, &focus);
            });
        }
    });
}

/// PoB-layout renderer: lays the section list out in three columns by group
/// (1 = Offence, 2 = Core, 3 = Defence) and, inside each section, a collapsible
/// per-subsection grid of (label, value) rows. Filter and hide-zero from
/// [`CalcsTabState`] still apply: the filter substring matches against
/// `section.id` / `subsection.label` / `row.label` / `row.output_key`,
/// and hide-zero hides rows whose `output_key` resolves to a zero value.
///
/// Slice 2 keeps two large simplifications vs upstream:
/// * Skill-flag visibility (`flag = "spell"`, `notFlag = "attack"`) is not
///   evaluated against the active skill — every row is shown — so single-handed
///   builds may see "OH …" rows and triggered-skill builds may see attack-time
///   rows. Empty values render as "—" so the noise is at least obviously
///   irrelevant. Slice 3 will wire the active-skill flag set in.
/// * Mod-only rows (`{0:mod:1}%` formats with no `output_key`) render as
///   "—" too — the breakdown port lands later.
fn render_pob_layout(
    ui: &mut egui::Ui,
    state: &mut CalcsTabState,
    sections: &[CalcSection],
    output: &Output,
    env: Option<&Env>,
    active_skill_flags: &HashSet<String>,
) {
    let q = state.filter.trim().to_lowercase();
    // Bucket sections by their column-group (1 = Offence, 2 = Core, 3 = Defence).
    // PoB orders sections within a group by their declaration order, so we preserve that.
    let mut by_group: [Vec<&CalcSection>; 3] = Default::default();
    for s in sections {
        let g = (s.group.saturating_sub(1).min(2)) as usize;
        by_group[g].push(s);
    }

    ui.horizontal(|ui| {
        // Left pane: 3-column section grid.
        ui.vertical(|ui| {
            let breakdown_open = state.focused_stat.is_some();
            if breakdown_open {
                ui.set_max_width(820.0);
            }
            egui::ScrollArea::vertical()
                .id_salt("calcs_pob_layout")
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    ui.horizontal_top(|ui| {
                        for (col_idx, col) in by_group.iter().enumerate() {
                            ui.vertical(|ui| {
                                ui.set_min_width(260.0);
                                ui.set_max_width(360.0);
                                ui.label(
                                    egui::RichText::new(group_heading(col_idx))
                                        .strong()
                                        .underline(),
                                );
                                ui.add_space(2.0);
                                for section in col {
                                    render_section(
                                        ui,
                                        section,
                                        output,
                                        &q,
                                        state.hide_zero,
                                        active_skill_flags,
                                        &mut state.focused_stat,
                                    );
                                }
                            });
                            if col_idx < 2 {
                                ui.separator();
                            }
                        }
                    });
                });
        });

        // Right pane: breakdown.
        if let Some(focus) = state.focused_stat.clone() {
            ui.separator();
            ui.vertical(|ui| {
                render_focused_breakdown(ui, env, &focus);
            });
        }
    });
}

fn group_heading(group: usize) -> &'static str {
    match group {
        0 => "OFFENCE",
        1 => "CORE",
        _ => "DEFENCE",
    }
}

/// Render one section: a stack of `egui::CollapsingHeader`s, one per subsection,
/// each housing a 2-column (label, value) grid.
fn render_section(
    ui: &mut egui::Ui,
    section: &CalcSection,
    output: &Output,
    filter_q: &str,
    hide_zero: bool,
    active_skill_flags: &HashSet<String>,
    focused: &mut Option<String>,
) {
    for sub in &section.subsections {
        let visible_rows: Vec<&CalcRow> = sub
            .rows
            .iter()
            .filter(|r| row_matches_filter(r, &section.id, &sub.label, filter_q))
            .filter(|r| row_passes_skill_flags(r, active_skill_flags))
            .filter(|r| {
                // hide_zero: drop rows whose resolvable output is zero. Rows with no
                // resolvable output_key are always kept so the layout stays meaningful.
                if !hide_zero {
                    return true;
                }
                match r.output_key.as_deref() {
                    Some(k) => output.try_get(k).is_some_and(|v| v.abs() > 1e-9),
                    None => false,
                }
            })
            .filter(|r| {
                // haveOutput visibility gate from PoB.
                match r.have_output.as_deref() {
                    Some(k) => output.try_get(k).is_some_and(|v| v.abs() > 1e-9),
                    None => true,
                }
            })
            .collect();
        if visible_rows.is_empty() {
            continue;
        }
        let header = if sub.label.is_empty() {
            section.id.clone()
        } else if sub.label == section.id {
            sub.label.clone()
        } else {
            format!("{}: {}", section.id, sub.label)
        };
        let id = egui::Id::new(("pob_calc_section", &section.id, &sub.label));
        egui::CollapsingHeader::new(header)
            .id_salt(id)
            .default_open(!sub.default_collapsed)
            .show(ui, |ui| {
                egui::Grid::new(id.with("grid"))
                    .num_columns(2)
                    .striped(true)
                    .show(ui, |ui| {
                        for row in visible_rows {
                            render_calc_row(ui, row, output, focused);
                        }
                    });
            });
    }
}

/// PoB-style skill-flag visibility gate. A row's `flag` field is a comma-joined list of
/// flag names (`spell`, `attack`, `weapon1Attack`, `warcry`, …); the row should appear
/// only when the active skill carries at least one of those flags. `not_flag` is the
/// inverse — the row is hidden when *any* listed flag is on the active skill.
///
/// When `active_skill_flags` is empty (no main skill bound), rows with `flag` set are
/// hidden — the same way PoB suppresses skill-specific rows on a fresh build.
fn row_passes_skill_flags(row: &CalcRow, active: &HashSet<String>) -> bool {
    if let Some(flag) = row.flag.as_deref() {
        let any = flag
            .split(',')
            .map(str::trim)
            .any(|f| !f.is_empty() && active.contains(f));
        if !any {
            return false;
        }
    }
    if let Some(not_flag) = row.not_flag.as_deref() {
        let any = not_flag
            .split(',')
            .map(str::trim)
            .any(|f| !f.is_empty() && active.contains(f));
        if any {
            return false;
        }
    }
    true
}

/// Derive the set of PoB-style skill flags that apply to the bound main skill. The set
/// names mirror the keys PoB uses in `skillFlags` (`attack`, `spell`, `warcry`, `area`,
/// `projectile`, `melee`, `triggered`, …) plus the synthetic `weapon1Attack` /
/// `weapon2Attack` / `bothWeaponAttack` markers attack rows expect.
///
/// Slice 3 of [#34](https://github.com/jonatanferm/PathOfBuildingMK2/issues/34) is
/// deliberately conservative: we mirror the skill's `base_flags` straight through and
/// always set `weapon1Attack` + `bothWeaponAttack` on attack skills (since MK2 always
/// runs the per-cast pass against the main hand). Off-hand-only context, channel-state,
/// triggered-by-CWDT, and per-element flag context are all follow-ups.
#[must_use]
pub fn active_skill_flags(character: &Character, skills: &SkillRegistry) -> HashSet<String> {
    let mut out = HashSet::new();
    let Some(main) = character.main_skill.as_ref() else {
        return out;
    };
    let Some(skill) = skills.get(&main.skill_id) else {
        return out;
    };
    for (k, v) in &skill.base_flags {
        if *v {
            out.insert(k.clone());
        }
    }
    // PoB's "weapon1Attack" / "bothWeaponAttack" markers are context-derived, not
    // base-flag values — set them ourselves so the matching attack rows in CalcSections
    // become visible. We assume a main-hand cast (MK2 doesn't yet split per-hand passes
    // for these calcs).
    if out.contains("attack") {
        out.insert("weapon1Attack".to_owned());
        out.insert("bothWeaponAttack".to_owned());
    }
    out
}

/// Substring filter against the row + section + subsection labels and the row's
/// output key. Empty filter matches everything.
fn row_matches_filter(row: &CalcRow, section_id: &str, sub_label: &str, q: &str) -> bool {
    if q.is_empty() {
        return true;
    }
    let hay = [
        section_id,
        sub_label,
        row.label.as_str(),
        row.output_key.as_deref().unwrap_or(""),
    ];
    hay.iter().any(|s| s.to_lowercase().contains(q))
}

fn render_calc_row(
    ui: &mut egui::Ui,
    row: &CalcRow,
    output: &Output,
    focused: &mut Option<String>,
) {
    let label_text = if row.label.is_empty() {
        row.output_key.as_deref().unwrap_or("(unnamed)").to_owned()
    } else {
        row.label.clone()
    };
    let label =
        ui.add(egui::Label::new(egui::RichText::new(&label_text)).sense(egui::Sense::click()));
    if label.clicked() {
        if let Some(key) = &row.output_key {
            *focused = Some(key.clone());
        }
    }
    if label.hovered() && row.output_key.is_some() {
        label.on_hover_text(format!(
            "{} — click to see contributing modifiers",
            row.output_key.as_deref().unwrap_or(""),
        ));
    }
    ui.with_layout(egui::Layout::right_to_left(egui::Align::Min), |ui| {
        let value = row.output_key.as_deref().and_then(|k| output.try_get(k));
        match value {
            Some(v) if v.abs() > 1e-9 => {
                ui.monospace(format_value(v));
            }
            _ => {
                ui.weak("—");
            }
        }
    });
    ui.end_row();
}

fn format_value(v: f64) -> String {
    if v.fract().abs() < 1e-9 {
        format!("{v:>10.0}")
    } else if v.abs() < 100.0 {
        format!("{v:>10.4}")
    } else {
        format!("{v:>10.2}")
    }
}

fn render_row(ui: &mut egui::Ui, k: &str, v: f64, focused: &mut Option<String>) {
    let label_text = egui::RichText::new(k).monospace();
    let label = ui.add(egui::Label::new(label_text).sense(egui::Sense::click()));
    if label.clicked() {
        *focused = Some(k.to_owned());
    }
    if label.hovered() {
        label.on_hover_text("Click to see contributing modifiers");
    }
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

/// Top-level breakdown panel. Tries the engine's [`derive_for`] helper
/// first (which produces a step-by-step CalcBreakdown.lua-style derivation
/// for Damage / Speed / Crit) and falls back to the legacy contributing-
/// modifiers view when no custom breakdown is registered.
fn render_focused_breakdown(ui: &mut egui::Ui, env: Option<&Env>, stat: &str) {
    ui.heading(stat);
    let Some(env) = env else {
        ui.weak("ModDB unavailable.");
        return;
    };
    if let Some(breakdown) = derive_for(env, stat) {
        ui.weak("derivation");
        ui.separator();
        render_step_breakdown(ui, &breakdown);
        ui.add_space(4.0);
        ui.collapsing("Raw modifiers", |ui| {
            render_breakdown(ui, env, stat);
        });
    } else {
        ui.weak("contributing modifiers");
        ui.separator();
        render_breakdown(ui, env, stat);
    }
}

/// Render the engine-derived [`Breakdown`] as one row per step with
/// optional contributing-source rollouts under each step.
fn render_step_breakdown(ui: &mut egui::Ui, breakdown: &Breakdown) {
    egui::ScrollArea::vertical()
        .id_salt("calcs_step_breakdown")
        .auto_shrink([false, false])
        .max_height(420.0)
        .show(ui, |ui| {
            for step in &breakdown.steps {
                render_one_step(ui, step);
            }
            ui.separator();
            ui.horizontal(|ui| {
                ui.label(egui::RichText::new("Total").strong());
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Min), |ui| {
                    ui.monospace(format_value(breakdown.total));
                });
            });
        });
}

fn render_one_step(ui: &mut egui::Ui, step: &BreakdownStep) {
    ui.horizontal(|ui| {
        ui.label(egui::RichText::new(&step.label).strong());
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Min), |ui| {
            if let Some(v) = step.value {
                ui.monospace(format_step_value(v));
            }
        });
    });
    if let Some(explain) = &step.explain {
        ui.weak(format!("    {explain}"));
    }
    if !step.sources.is_empty() {
        // Inline-collapse the source list so a stat with 30 contributors
        // doesn't dominate the panel — matches PoB's behaviour where the
        // breakdown is concise by default and the user expands a section
        // for the long-tail mods.
        let id = egui::Id::new(("step_sources", &step.label));
        egui::CollapsingHeader::new(format!("    {} sources", step.sources.len()))
            .id_salt(id)
            .default_open(false)
            .show(ui, |ui| {
                egui::Grid::new(id.with("grid"))
                    .num_columns(3)
                    .striped(true)
                    .show(ui, |ui| {
                        for src in &step.sources {
                            render_mod_source_row(ui, src);
                        }
                    });
            });
    }
    ui.add_space(2.0);
}

fn render_mod_source_row(ui: &mut egui::Ui, src: &ModSource) {
    let kind = match src.kind {
        ModType::Base => "BASE",
        ModType::Inc => "INC",
        ModType::More => "MORE",
        ModType::Flag => "FLAG",
        ModType::Override => "OVERRIDE",
        ModType::List => "LIST",
        ModType::Max => "MAX",
        ModType::Min => "MIN",
    };
    ui.monospace(kind);
    if let Some(v) = src.value {
        ui.monospace(format!("{v:>+8.2}"));
    } else {
        ui.monospace("       —");
    }
    ui.label(&src.source);
    ui.end_row();
}

fn format_step_value(v: f64) -> String {
    // Step values can be raw scalars (1.30 cast speed mult), small
    // percentages (0.06 crit chance), or large numerics (1650 DPS).
    // Pick a precision that reads cleanly in each regime.
    if v.fract().abs() < 1e-9 {
        format!("{v:>10.0}")
    } else if v.abs() < 10.0 {
        format!("{v:>10.4}")
    } else if v.abs() < 1000.0 {
        format!("{v:>10.2}")
    } else {
        format!("{v:>10.0}")
    }
}

/// Walk env.mod_db for mods named `stat` and render them in groups by ModType.
/// PoB-style breakdown: BASE adders, INC totals, MORE multipliers, FLAGs, OVERRIDEs.
fn render_breakdown(ui: &mut egui::Ui, env: &Env, stat: &str) {
    let mods: Vec<&Mod> = env.mod_db.iter_named(stat).collect();
    if mods.is_empty() {
        ui.weak(format!("No mods directly named `{stat}`."));
        ui.add_space(4.0);
        ui.weak(
            "(Some outputs are derived from other outputs — e.g. EHP from Life + ES + resists \
             — so they have no direct contributing mods. Try a base stat like Life, Mana, \
             FireResist, or Strength to see the contributing list.)",
        );
        return;
    }
    // Group by kind in a fixed order so the breakdown reads top-down
    // BASE → INC → MORE → FLAG → OVERRIDE → LIST → MAX → MIN.
    const ORDER: &[ModType] = &[
        ModType::Base,
        ModType::Inc,
        ModType::More,
        ModType::Flag,
        ModType::Override,
        ModType::List,
        ModType::Max,
        ModType::Min,
    ];
    let mut by_kind: Vec<(ModType, Vec<&Mod>)> = ORDER.iter().map(|k| (*k, Vec::new())).collect();
    for m in mods {
        if let Some(slot) = by_kind.iter_mut().find(|(k, _)| *k == m.kind) {
            slot.1.push(m);
        }
    }
    by_kind.retain(|(_, v)| !v.is_empty());

    egui::ScrollArea::vertical()
        .id_salt("calcs_breakdown")
        .auto_shrink([false, false])
        .max_height(420.0)
        .show(ui, |ui| {
            for (kind, list) in &by_kind {
                let kind = *kind;
                ui.label(egui::RichText::new(kind_label(kind)).strong());
                egui::Grid::new(format!("breakdown_{kind:?}"))
                    .num_columns(2)
                    .striped(true)
                    .show(ui, |ui| {
                        for m in list {
                            render_mod_row(ui, m);
                        }
                    });
                ui.add_space(6.0);
            }
        });
}

fn render_mod_row(ui: &mut egui::Ui, m: &Mod) {
    let value = match &m.value {
        pob_engine::ModValue::Number(n) => format!("{n:>+8.2}"),
        pob_engine::ModValue::Range { min, max } => format!("{min:.0}-{max:.0}"),
        pob_engine::ModValue::Bool(b) => format!("{b}"),
        pob_engine::ModValue::Str(s) => s.clone(),
    };
    ui.monospace(value);
    let source_label = source_label(m.source.as_ref());
    let mut tags = String::new();
    for t in &m.tags {
        if !tags.is_empty() {
            tags.push_str(" · ");
        }
        tags.push_str(&format_tag(t));
    }
    let mut text = source_label;
    if !tags.is_empty() {
        text.push_str(" — ");
        text.push_str(&tags);
    }
    ui.label(text);
    ui.end_row();
}

fn source_label(s: Option<&Source>) -> String {
    match s {
        Some(Source::Tree) => "tree".into(),
        Some(Source::Passive(id)) => format!("passive: #{id}"),
        Some(Source::Ascendancy(s)) => format!("ascendancy: {s}"),
        Some(Source::Item(slot)) => format!("item slot {slot}"),
        Some(Source::Skill(id)) => format!("skill: {id}"),
        Some(Source::Buff(id)) => format!("buff: {id}"),
        Some(Source::Config(id)) => format!("config: {id}"),
        Some(Source::Other(s)) => s.clone(),
        None => "(unknown)".into(),
    }
}

fn format_tag(t: &Tag) -> String {
    match &t.kind {
        pob_engine::TagKind::Condition { var, neg } => {
            if *neg {
                format!("not cond:{var}")
            } else {
                format!("cond:{var}")
            }
        }
        pob_engine::TagKind::ActorCondition { actor, var, .. } => {
            format!("{actor}:{var}")
        }
        pob_engine::TagKind::Multiplier { var, .. } => format!("mult:{var}"),
        pob_engine::TagKind::PerStat { stat, .. } => format!("per:{stat}"),
        pob_engine::TagKind::PercentStat { stat, percent } => {
            format!("{percent}% of {stat}")
        }
        pob_engine::TagKind::StatThreshold {
            stat,
            threshold,
            upper,
        } => {
            let cmp = if *upper { "<" } else { ">=" };
            format!("if:{stat}{cmp}{threshold}")
        }
        pob_engine::TagKind::MultiplierThreshold {
            var,
            threshold,
            upper,
        } => {
            let cmp = if *upper { "<" } else { ">=" };
            format!("if:{var}{cmp}{threshold}")
        }
        pob_engine::TagKind::SkillName { skill_name, .. } => format!("skill:{skill_name}"),
        pob_engine::TagKind::SkillType { skill_type, .. } => format!("type:{skill_type}"),
        pob_engine::TagKind::SkillId { skill_id, .. } => format!("id:{skill_id}"),
        pob_engine::TagKind::SlotName { slot_name, .. } => format!("slot:{slot_name}"),
        pob_engine::TagKind::Unknown(_) => "unknown".into(),
    }
}

fn kind_label(k: ModType) -> &'static str {
    match k {
        ModType::Base => "BASE",
        ModType::Inc => "INC %",
        ModType::More => "MORE %",
        ModType::Flag => "FLAG",
        ModType::Override => "OVERRIDE",
        ModType::List => "LIST",
        ModType::Max => "MAX",
        ModType::Min => "MIN",
    }
}

#[cfg(test)]
mod tests {
    use super::GROUPS;

    /// Returns the first group heading whose patterns match `key`, or
    /// `None` if it falls through to "Other". Matches the same shape
    /// as the runtime grouping code (substring match, with `:` suffix
    /// handled as a strict prefix).
    fn group_for(key: &str) -> Option<&'static str> {
        for (heading, patterns) in GROUPS {
            for p in *patterns {
                let hit = if p.ends_with(':') {
                    key.starts_with(p)
                } else {
                    key.contains(p)
                };
                if hit {
                    return Some(*heading);
                }
            }
        }
        None
    }

    #[test]
    fn warcry_outputs_land_under_warcry_section() {
        // Slice 3-6 outputs all need the dedicated Warcry section so
        // they don't end up in the "Other" overflow at the bottom.
        // Slice 7 added the per-cry `<Cry>Active` markers; slices 8-10,
        // 12 and 16 added the per-cry buff outputs (LifeRegenPct,
        // ResistBonus, ArmourBonus, etc.). All of them carry "Cry"
        // or "Warcry" or "Exerted" substrings and must land in the
        // dedicated section.
        for key in [
            // Slice 3 aggregates.
            "ActiveWarcryCount",
            "WarcryExertedAttackCountTotal",
            "WarcryMinCooldown",
            // Slice 2 config knob.
            "WarcryPower",
            // Slice 4 auto-uptime.
            "ExertedAttackUptime",
            "ExertedAttackDamageBonus",
            // Slice 7 per-cry active markers.
            "IntimidatingCryActive",
            "EnduringCryActive",
            "AncestralCryActive",
            "SeismicCryActive",
            "BattlemagesCryActive",
            "RallyingCryActive",
            "InfernalCryActive",
            "GeneralsCryActive",
            // Slice 8: Enduring Cry life regen.
            "EnduringCryLifeRegenPct",
            // Slice 9: Ancestral Cry resists.
            "AncestralCryResistBonus",
            "AncestralCryMaxResistBonus",
            // Slice 10: Seismic Cry armour + stun threshold.
            "SeismicCryArmourBonus",
            "SeismicCryStunThresholdBonus",
            // Slice 12: Battlemage's Cry crit chance.
            "BattlemagesCryCritBonus",
            // Slice 16: Rallying Cry per-ally exert.
            "RallyingCryExertDamageBonus",
            "RallyingCryAllyCount",
            // Issue #145 (slice 1): Rallying Cry ally weapon-damage projection.
            "RallyingCryAllyWeaponDamageBonus",
            "RallyingCryAllyWeaponDamageTotal",
            // Issue #145 (slice 3): Infernal Cry phys-as-fire.
            "InfernalCryGainAsFireBonus",
            // Issue #145 (slice 4): General's Cry mirage envelope.
            "GeneralsCryMirageCount",
            "GeneralsCryCooldown",
            "GeneralsCryCastsPerSecond",
            "GeneralsCryDpsContribution",
            "GeneralsCryLevel",
        ] {
            let group = group_for(key);
            assert_eq!(
                group,
                Some("Warcry"),
                "{key} should bucket under Warcry, got {group:?}"
            );
        }
    }

    #[test]
    fn pob_layout_filter_matches_against_section_subsection_and_label() {
        use pob_data::CalcRow;
        let row = CalcRow {
            label: "Attacks per second".to_owned(),
            output_key: Some("Speed".to_owned()),
            have_output: None,
            format: None,
            flag: None,
            not_flag: None,
        };
        // Empty filter accepts everything.
        assert!(super::row_matches_filter(
            &row,
            "Speed",
            "Attack/Cast Rate",
            ""
        ));
        // Filter against the row label.
        assert!(super::row_matches_filter(
            &row,
            "Speed",
            "Attack/Cast Rate",
            "attacks"
        ));
        // Filter against the section id.
        assert!(super::row_matches_filter(
            &row,
            "Speed",
            "Attack/Cast Rate",
            "speed"
        ));
        // Filter against the subsection label.
        assert!(super::row_matches_filter(
            &row,
            "Speed",
            "Attack/Cast Rate",
            "cast"
        ));
        // Filter against the output key.
        assert!(super::row_matches_filter(
            &row,
            "Speed",
            "Attack/Cast Rate",
            "speed"
        ));
        // Non-matching filter rejects.
        assert!(!super::row_matches_filter(
            &row,
            "Speed",
            "Attack/Cast Rate",
            "lightning"
        ));
    }

    #[test]
    fn pob_layout_value_format_picks_compact_precision() {
        // Whole-integer outputs render zero fractional digits.
        assert_eq!(super::format_value(123.0).trim(), "123");
        // Small-magnitude outputs get the fine-grained 4-digit format.
        assert!(super::format_value(0.5).trim().starts_with("0.5"));
        // Large outputs round to 2 decimals.
        assert_eq!(super::format_value(12345.6789).trim(), "12345.68");
    }

    #[test]
    fn pob_layout_skill_flags_hide_attack_only_rows_for_spells() {
        use pob_data::CalcRow;
        use std::collections::HashSet;

        let attack_row = CalcRow {
            label: "MH Att. Speed".into(),
            output_key: Some("MainHand.Speed".into()),
            have_output: None,
            format: None,
            flag: Some("weapon1Attack".into()),
            not_flag: Some("triggered".into()),
        };
        let spell_row = CalcRow {
            label: "Casts per second".into(),
            output_key: Some("Speed".into()),
            have_output: None,
            format: None,
            flag: Some("spell".into()),
            not_flag: Some("triggered".into()),
        };
        let warcry_row = CalcRow {
            label: "Uses per second".into(),
            output_key: Some("Speed".into()),
            have_output: None,
            format: None,
            flag: Some("warcry".into()),
            not_flag: None,
        };
        let unconditional_row = CalcRow {
            label: "Inc. Att. Speed".into(),
            output_key: None,
            have_output: None,
            format: None,
            flag: None,
            not_flag: None,
        };

        // Spell-flagged build: hides attack-only rows, shows spell rows.
        let mut spell: HashSet<String> = HashSet::new();
        spell.insert("spell".into());
        spell.insert("hit".into());
        assert!(!super::row_passes_skill_flags(&attack_row, &spell));
        assert!(super::row_passes_skill_flags(&spell_row, &spell));
        assert!(!super::row_passes_skill_flags(&warcry_row, &spell));
        assert!(super::row_passes_skill_flags(&unconditional_row, &spell));

        // Attack build: shows the attack row, hides the spell row.
        let mut attack: HashSet<String> = HashSet::new();
        attack.insert("attack".into());
        attack.insert("weapon1Attack".into());
        attack.insert("hit".into());
        assert!(super::row_passes_skill_flags(&attack_row, &attack));
        assert!(!super::row_passes_skill_flags(&spell_row, &attack));

        // Triggered attack: notFlag = "triggered" should hide the row even though it
        // matches `weapon1Attack`.
        let mut triggered_attack: HashSet<String> = HashSet::new();
        triggered_attack.insert("attack".into());
        triggered_attack.insert("weapon1Attack".into());
        triggered_attack.insert("triggered".into());
        assert!(!super::row_passes_skill_flags(
            &attack_row,
            &triggered_attack
        ));

        // No active skill: rows with `flag` set are hidden; unconditional rows still
        // render so the layout doesn't go blank on a fresh build.
        let none: HashSet<String> = HashSet::new();
        assert!(!super::row_passes_skill_flags(&attack_row, &none));
        assert!(!super::row_passes_skill_flags(&spell_row, &none));
        assert!(super::row_passes_skill_flags(&unconditional_row, &none));
    }

    #[test]
    fn pob_layout_flag_supports_comma_separated_lists() {
        use pob_data::CalcRow;
        use std::collections::HashSet;

        // PoB sometimes targets multiple flags via flagList = {"a", "b"}; the extractor
        // joins those with commas. The visibility check has to OR them.
        let row = CalcRow {
            label: "Trigger Rate Cap".into(),
            output_key: Some("TriggerRateCap".into()),
            have_output: None,
            format: None,
            flag: Some("triggered,hasOverride".into()),
            not_flag: Some("focused,skipEffectiveRate".into()),
        };

        let mut active: HashSet<String> = HashSet::new();
        active.insert("triggered".into());
        // hasOverride absent — but `flag` is OR, so triggered alone is enough.
        assert!(super::row_passes_skill_flags(&row, &active));

        // notFlag matching focused → hide.
        active.insert("focused".into());
        assert!(!super::row_passes_skill_flags(&row, &active));
    }

    #[test]
    fn minion_outputs_land_under_minion_section() {
        // Issue #20 slices 3-6: every minion-side output the engine emits today
        // must bucket under the dedicated "Minion" group so it's not absorbed by
        // the generic Life / Damage / Resists / Crits patterns.
        for key in [
            // Slice 3: detection + life / resists.
            "MinionLifeBase",
            "MinionLife",
            "MinionFireResist",
            "MinionColdResist",
            "MinionLightningResist",
            "MinionChaosResist",
            // Slice 5: damage + attack rate + DPS.
            "MinionDamageBase",
            "MinionAverageDamage",
            "MinionMinDamage",
            "MinionMaxDamage",
            "MinionAttacksPerSecondBase",
            "MinionAttacksPerSecond",
            "MinionDPS",
            // Slice 6: resist breakdown + crit factor.
            "MinionFireResistBase",
            "MinionColdResistBase",
            "MinionLightningResistBase",
            "MinionChaosResistBase",
            "MinionCritChance",
            "MinionCritMultiplier",
            // Slice 11: life regen rate.
            "MinionLifeRegenPercent",
            "MinionLifeRegen",
        ] {
            let group = group_for(key);
            assert_eq!(
                group,
                Some("Minion"),
                "{key} should bucket under Minion, got {group:?}"
            );
        }
    }

    #[test]
    fn mine_and_trap_outputs_land_under_mines_traps_section() {
        // Slice 4's TrapCooldown / MineCooldown plus the slice 1-3
        // throw-rate outputs all need a dedicated Mines / Traps
        // section so the user can scan them in one place.
        for key in [
            "MineLayingTime",
            "MineLayingSpeed",
            "MineCooldown",
            "NumberOfMines",
            "MinesPlaced",
            "TrapThrowingTime",
            "TrapThrowingSpeed",
            "TrapCooldown",
            "NumberOfTraps",
        ] {
            let group = group_for(key);
            assert_eq!(
                group,
                Some("Mines / Traps"),
                "{key} should bucket under Mines / Traps, got {group:?}"
            );
        }
    }
}
