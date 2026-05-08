//! Calcs tab — flat dump of every computed output stat plus a click-to-drill-down
//! panel that walks the contributing modifiers from the live ModDB.

use eframe::egui;
use pob_engine::{Env, Mod, ModStore as _, ModType, Output, Source, Tag};

#[derive(Default)]
pub struct CalcsTabState {
    pub filter: String,
    pub hide_zero: bool,
    /// Stat the user clicked to inspect. `None` collapses the breakdown panel.
    pub focused_stat: Option<String>,
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

pub fn ui(ui: &mut egui::Ui, state: &mut CalcsTabState, output: &Output, env: Option<&Env>) {
    ui.horizontal(|ui| {
        ui.label("Filter:");
        ui.add(
            egui::TextEdit::singleline(&mut state.filter)
                .desired_width(220.0)
                .hint_text("Life, FireResist, MainSkill, …"),
        );
        ui.checkbox(&mut state.hide_zero, "Hide zero values");
        ui.separator();
        ui.label(format!("{} stats", output.len()));
        if state.focused_stat.is_some() {
            if ui.button("Close breakdown").clicked() {
                state.focused_stat = None;
            }
        }
    });
    ui.separator();

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
                ui.heading(&focus);
                ui.weak("contributing modifiers");
                ui.separator();
                if let Some(env) = env {
                    render_breakdown(ui, env, &focus);
                } else {
                    ui.weak("ModDB unavailable.");
                }
            });
        }
    });
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
        for key in [
            "ActiveWarcryCount",
            "WarcryExertedAttackCountTotal",
            "WarcryMinCooldown",
            "WarcryPower",
            "ExertedAttackUptime",
            "ExertedAttackDamageBonus",
            "IntimidatingCryActive",
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
