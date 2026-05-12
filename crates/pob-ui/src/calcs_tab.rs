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

/// Issue #34 follow-up: ordering mode for the flat-view entry list.
/// `Key` mirrors the pre-#34 alphabetical default; the two value
/// modes help users quickly spot the biggest / smallest outputs in
/// the dictionary without scanning the entire alphabetised list.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum CalcsFlatSort {
    /// Alphabetical by output key (case-insensitive isn't necessary
    /// since engine keys are PascalCase identifiers).
    #[default]
    Key,
    /// Largest value first. NaN sinks to the bottom so a stray engine
    /// NaN can't pin garbage to the top.
    ValueDesc,
    /// Smallest value first. Mirror of `ValueDesc` — useful for
    /// finding "where am I leaking damage" / "what's at zero".
    ValueAsc,
}

impl CalcsFlatSort {
    /// Human-readable label for the UI selector.
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::Key => "Name",
            Self::ValueDesc => "Value ↓",
            Self::ValueAsc => "Value ↑",
        }
    }
}

/// Issue #34 follow-up: sort `entries` in place under the chosen
/// [`CalcsFlatSort`] mode. Pure helper so the value-axis NaN handling
/// has a unit-test home; the UI side just picks the mode.
pub fn sort_flat_entries(entries: &mut [(&str, f64)], mode: CalcsFlatSort) {
    match mode {
        CalcsFlatSort::Key => entries.sort_by(|a, b| a.0.cmp(b.0)),
        CalcsFlatSort::ValueDesc => entries.sort_by(|a, b| {
            // NaN sinks: treat NaN as the smallest finite value's
            // neighbour-from-below. Same shape as
            // `truncate_to_top_n` in node_power_heatmap.
            match (a.1.is_nan(), b.1.is_nan()) {
                (true, true) => a.0.cmp(b.0),
                (true, false) => std::cmp::Ordering::Greater,
                (false, true) => std::cmp::Ordering::Less,
                (false, false) => {
                    b.1.partial_cmp(&a.1)
                        .unwrap_or(std::cmp::Ordering::Equal)
                        .then_with(|| a.0.cmp(b.0))
                }
            }
        }),
        CalcsFlatSort::ValueAsc => entries.sort_by(|a, b| match (a.1.is_nan(), b.1.is_nan()) {
            (true, true) => a.0.cmp(b.0),
            (true, false) => std::cmp::Ordering::Greater,
            (false, true) => std::cmp::Ordering::Less,
            (false, false) => {
                a.1.partial_cmp(&b.1)
                    .unwrap_or(std::cmp::Ordering::Equal)
                    .then_with(|| a.0.cmp(b.0))
            }
        }),
    }
}

#[derive(Default)]
pub struct CalcsTabState {
    pub filter: String,
    pub hide_zero: bool,
    /// Issue #34 follow-up: ordering mode for the flat-view entry
    /// list. Defaults to `Key` (alphabetical) — the pre-this-slice
    /// behaviour.
    pub flat_sort: CalcsFlatSort,
    /// Stat the user clicked to inspect. `None` collapses the breakdown panel.
    pub focused_stat: Option<String>,
    /// When `true` and `calc_sections` is loaded, render the PoB-style grouped
    /// section layout instead of the flat key list.
    pub use_pob_layout: bool,
    /// Issue #207 follow-up: LRU of stat keys the user has recently
    /// drilled into. Latest at front, dedup'd, capped at
    /// [`RECENTLY_FOCUSED_MAX`] entries — drives a chip row at the top
    /// of the tab so power users can jump back to a stat they were
    /// iterating on without re-typing the filter.
    pub recently_focused: std::collections::VecDeque<String>,
}

/// Issue #207 follow-up: cap on how many entries
/// [`CalcsTabState::recently_focused`] holds. Five fits in a single
/// chip row at typical font sizes and matches the working-memory
/// span of "what was I just looking at?".
pub const RECENTLY_FOCUSED_MAX: usize = 5;

/// Issue #34 follow-up: reset the user's in-flight Calcs-view state.
/// Clears the filter input, the breakdown focus, the recently-focused
/// chip row, and the flat-view sort. Preserves the two preferences
/// the user explicitly opts into (`hide_zero`, `use_pob_layout`) —
/// those stay sticky across resets so a power user who configured
/// their preferred layout doesn't lose it.
pub fn reset_calcs_view(state: &mut CalcsTabState) {
    state.filter.clear();
    state.focused_stat = None;
    state.recently_focused.clear();
    state.flat_sort = CalcsFlatSort::default();
}

/// Issue #34 follow-up: count how many output entries survive the
/// active filter + hide-zero rule. Drives the header chip's "X of Y
/// stats" readout so the user can tell at a glance whether a filter
/// is hiding most rows vs. simply showing them all.
///
/// Mirrors the filter logic used inline by the flat-view renderer —
/// keep the two in sync if either rule changes.
///
/// Pure / no-egui so the count contract is unit-testable in isolation.
#[must_use]
pub fn count_filtered_output_entries(output: &Output, filter: &str, hide_zero: bool) -> usize {
    let q = filter.trim().to_ascii_lowercase();
    output
        .iter()
        .filter(|(k, _)| q.is_empty() || k.to_ascii_lowercase().contains(&q))
        .filter(|(_, v)| !hide_zero || v.abs() > 1e-9)
        .count()
}

/// Issue #207 follow-up: push `stat` to the front of `deque` LRU-style
/// — if it's already in there, the existing entry moves to the front
/// instead of duplicating. The deque is truncated to `max_len` after
/// the push so the chip row never grows past the configured cap.
///
/// Empty `stat` is silently dropped (defensive against an accidental
/// blank-key focus through the breakdown side panel).
pub fn push_recent_stat(
    deque: &mut std::collections::VecDeque<String>,
    stat: &str,
    max_len: usize,
) {
    if stat.is_empty() {
        return;
    }
    if let Some(idx) = deque.iter().position(|s| s == stat) {
        deque.remove(idx);
    }
    deque.push_front(stat.to_owned());
    while deque.len() > max_len {
        deque.pop_back();
    }
}

/// Stat category groupings. Each entry maps a heading to the substring
/// patterns that route output keys into that group, plus the PoB column
/// (0 = Offence, 1 = Core, 2 = Defence) it lays out in.
///
/// Section order + names track upstream PoB's `Modules/CalcSections.lua`
/// structure: Offence groups first (Skill Hit Damage / Speed / Crit /
/// Accuracy / Impale / Bleed / Poison / Ignite / Other Effects), then
/// Attributes, then Defence (Resists / Damage Avoidance / Charges /
/// Other Defences). A full CalcSections.lua port (#34) keeps section-row
/// breakdowns scoped to each stat, but matching the layout already gets
/// us most of the usability win.
///
/// Order matters for substring routing: groups with narrow prefixes
/// (`Warcry`, `Mines / Traps`, `Minion`) must come before the generic
/// offence patterns so keys like `MinionLife` don't get absorbed by the
/// `Life` substring under "Pools".
struct Group {
    heading: &'static str,
    patterns: &'static [&'static str],
    /// 0 = Offence, 1 = Core, 2 = Defence — drives the three-column layout.
    column: u8,
}

const GROUPS: &[Group] = &[
    // Issue #19: warcry / exertion outputs. Listed first so keys
    // like `ExertedAttackDamageBonus` don't get swept into the
    // generic "Skill Hit Damage" group by the substring match.
    Group {
        heading: "Warcry",
        patterns: &["Warcry", "Exerted", "Cry"],
        column: 0,
    },
    // Issue #84: mine / trap timing outputs. Listed before
    // "Attack / Cast Rate" so `MineLayingSpeed` /
    // `TrapThrowingSpeed` aren't absorbed by the generic "Speed"
    // pattern.
    Group {
        heading: "Mines / Traps",
        patterns: &["Mine", "Trap"],
        column: 0,
    },
    // Issue #20 (slices 3-6): minion outputs. Listed before
    // "Skill Hit Damage" / "Pools" / "Resists" so keys like
    // `MinionLife` / `MinionFireResist` / `MinionDPS` don't get
    // absorbed by the generic `Life` / `FireResist` / `Damage`
    // patterns.
    Group {
        heading: "Minion",
        patterns: &["Minion"],
        column: 0,
    },
    // OFFENCE column.
    Group {
        heading: "Skill Hit Damage",
        patterns: &[
            "MainSkill",
            "FullDPS",
            "WithBleedDPS",
            "WithImpaleDPS",
            "Damage",
        ],
        column: 0,
    },
    Group {
        heading: "Attack / Cast Rate",
        patterns: &["Speed", "AttackSpeed", "CastSpeed", "MainSkillSpeed"],
        column: 0,
    },
    Group {
        heading: "Crits",
        patterns: &["Crit", "CritChance", "CritMultiplier"],
        column: 0,
    },
    Group {
        heading: "Impale",
        patterns: &["Impale"],
        column: 0,
    },
    Group {
        heading: "Accuracy",
        patterns: &["Accuracy", "HitChance"],
        column: 0,
    },
    Group {
        heading: "Bleed",
        patterns: &["Bleed"],
        column: 0,
    },
    Group {
        heading: "Poison",
        patterns: &["Poison"],
        column: 0,
    },
    Group {
        heading: "Ignite",
        patterns: &["Ignite"],
        column: 0,
    },
    Group {
        heading: "Non-Damaging Ailments",
        patterns: &["Freeze", "Shock", "Chill", "Scorch", "Ailment"],
        column: 0,
    },
    Group {
        heading: "Other Offence",
        patterns: &["Projectile", "Chain", "AoE", "Area"],
        column: 0,
    },
    // CORE column.
    Group {
        heading: "Attributes",
        patterns: &["Strength", "Dexterity", "Intelligence", "AllAttributes"],
        column: 1,
    },
    Group {
        heading: "Pools",
        patterns: &["Life", "Mana", "EnergyShield", "Ward", "Rage"],
        column: 1,
    },
    // DEFENCE column.
    Group {
        heading: "Resists",
        patterns: &[
            "FireResist",
            "ColdResist",
            "LightningResist",
            "ChaosResist",
            "ElementalResist",
        ],
        column: 2,
    },
    Group {
        heading: "Damage Avoidance",
        patterns: &["Block", "Suppress", "Dodge", "Avoid"],
        column: 2,
    },
    Group {
        heading: "Charges",
        patterns: &["Charge", "PowerCharge", "FrenzyCharge", "EnduranceCharge"],
        column: 2,
    },
    Group {
        heading: "Other Defences",
        patterns: &[
            "Armour", "Evasion", "Recover", "Regen", "Recharge", "Phys", "EHP",
        ],
        column: 2,
    },
    Group {
        heading: "Misc",
        patterns: &["Misc:", "Keystone:"],
        column: 2,
    },
];

pub fn ui(
    ui: &mut egui::Ui,
    state: &mut CalcsTabState,
    output: &Output,
    env: Option<&Env>,
    calc_sections: Option<&[CalcSection]>,
    active_skill_flags: &HashSet<String>,
) {
    // Issue #207 follow-up: detect focused-stat changes at frame top
    // so the chip row updates LRU-style without threading `recent`
    // into every per-row renderer. We snapshot the previous value,
    // let the renderers run, then push the new value (if it changed)
    // through `push_recent_stat` afterwards.
    let prev_focused = state.focused_stat.clone();
    ui.horizontal(|ui| {
        ui.label("Filter:");
        ui.add(
            egui::TextEdit::singleline(&mut state.filter)
                .desired_width(220.0)
                .hint_text("Life, FireResist, MainSkill, …"),
        );
        ui.checkbox(&mut state.hide_zero, "Hide zero values");
        // Issue #34 follow-up: flat-view sort selector. Hidden when
        // PoB layout is on — the section view's per-row order comes
        // from the calc_sections.json structure, not this list.
        if !state.use_pob_layout {
            egui::ComboBox::from_id_salt("calcs_flat_sort")
                .selected_text(state.flat_sort.label())
                .show_ui(ui, |ui| {
                    for mode in [
                        CalcsFlatSort::Key,
                        CalcsFlatSort::ValueDesc,
                        CalcsFlatSort::ValueAsc,
                    ] {
                        ui.selectable_value(&mut state.flat_sort, mode, mode.label());
                    }
                })
                .response
                .on_hover_text(
                    "Order outputs by key (alphabetical) or by raw value. Value \
                     sort is handy for spotting the biggest / smallest stats \
                     without scrolling the alphabetised list.",
                );
        }
        if calc_sections.is_some() {
            ui.checkbox(&mut state.use_pob_layout, "PoB layout")
                .on_hover_text(
                    "Render the Calcs tab in PoB's three-column section layout, sourced from \
                     Modules/CalcSections.lua. Falls back to the flat key list when off.",
                );
        }
        ui.separator();
        // "X of Y stats" only when a filter is narrowing the flat list;
        // cold-open / no-filter keeps the historical bare "X stats"
        // form so the chip doesn't add noise to the common path. PoB-
        // layout mode filters CalcRows by section/subsection/label
        // rather than by raw Output keys, so the helper's count would
        // not match what the user sees — fall back to the bare total.
        let total = output.len();
        let flat_filtering =
            !state.use_pob_layout && (!state.filter.trim().is_empty() || state.hide_zero);
        let stats_label = if flat_filtering {
            let kept = count_filtered_output_entries(output, &state.filter, state.hide_zero);
            format!("{kept} of {total} stats")
        } else {
            format!("{total} stats")
        };
        ui.label(stats_label);
        if state.focused_stat.is_some() {
            if ui.button("Close breakdown").clicked() {
                state.focused_stat = None;
            }
        }
        // Issue #34 follow-up: one-click view reset. Clears the
        // filter, the breakdown focus, the recents chip row, and
        // the flat-view sort — but keeps the user's `Hide zero values`
        // and `PoB layout` preferences sticky.
        if ui
            .small_button("Reset view")
            .on_hover_text(
                "Clear the filter, close the breakdown, drop the recents chip row, \
                 and reset the flat-view sort. Keeps `Hide zero` and `PoB layout` \
                 preferences sticky.",
            )
            .clicked()
        {
            reset_calcs_view(state);
        }
        // Issue #34 follow-up: clipboard export of the output
        // dictionary. When a filter or hide_zero is active, the
        // exported text mirrors what the table shows row-for-row so a
        // user copying "all my resists" doesn't paste every stat
        // alongside them. Cold-open / no-filter path still copies the
        // full dictionary so the historical behaviour is preserved.
        // Same gating as the "X of Y stats" chip (#468): in PoB-layout
        // mode the on-screen filter runs over CalcRows by section /
        // subsection / label, NOT Output keys, so the helper would
        // copy a different set than the user sees. Fall back to the
        // full-dump "Copy output" path there.
        let filtering =
            !state.use_pob_layout && (!state.filter.trim().is_empty() || state.hide_zero);
        let copy_label = if filtering {
            "Copy filtered"
        } else {
            "Copy output"
        };
        let copy_hover = if filtering {
            "Copy the on-screen filtered output as `Key: value` plain text, \
             alphabetised by key. Reflects the live filter + Hide-zero \
             toggles."
        } else {
            "Copy every output stat as `Key: value` plain text, alphabetised \
             by key. Paste into a spreadsheet / Discord / GitHub issue."
        };
        if ui
            .add_enabled(!output.is_empty(), egui::Button::new(copy_label))
            .on_hover_text(copy_hover)
            .clicked()
        {
            let text = if filtering {
                format_output_filtered_as_text(output, &state.filter, state.hide_zero)
            } else {
                format_output_as_text(output)
            };
            ui.ctx().copy_text(text);
        }
    });
    // Issue #207 follow-up: chip row of the 5 most-recently inspected
    // stats. Clicking a chip re-focuses that stat. Closed-state UX
    // is "empty row" — once the user has drilled in once it stays
    // populated for the session.
    if !state.recently_focused.is_empty() {
        ui.horizontal(|ui| {
            ui.weak("Recent:");
            let recent: Vec<String> = state.recently_focused.iter().cloned().collect();
            for stat in recent {
                if ui
                    .small_button(&stat)
                    .on_hover_text("Re-focus this stat in the breakdown panel.")
                    .clicked()
                {
                    state.focused_stat = Some(stat.clone());
                }
            }
        });
    }
    ui.separator();

    if state.use_pob_layout && calc_sections.is_some() {
        if let Some(sections) = calc_sections {
            render_pob_layout(ui, state, sections, output, env, active_skill_flags);
        }
        // Issue #207 follow-up: also update the recents LRU for the
        // PoB-layout path. Mirrors the flat-layout tail below.
        update_recents_lru(state, prev_focused.as_deref());
        return;
    }

    let q = state.filter.trim().to_lowercase();
    let mut entries: Vec<(&str, f64)> = output.iter().collect();
    sort_flat_entries(&mut entries, state.flat_sort);
    let entries_filtered: Vec<(&str, f64)> = entries
        .into_iter()
        .filter(|(k, _)| q.is_empty() || k.to_lowercase().contains(&q))
        .filter(|(_, v)| !state.hide_zero || v.abs() > 1e-9)
        .collect();

    // Bucket each filtered entry into the first group whose pattern matches.
    // Iteration order of GROUPS is the priority order, so e.g. `MinionLife`
    // resolves to "Minion" before "Pools" can claim it via the `Life`
    // substring.
    let mut by_group: std::collections::HashMap<&str, Vec<(&str, f64)>> = Default::default();
    let mut leftovers: Vec<(&str, f64)> = Vec::new();
    for (k, v) in &entries_filtered {
        let mut matched = None;
        for g in GROUPS {
            if g.patterns.iter().any(|p| {
                if p.ends_with(':') {
                    k.starts_with(p)
                } else {
                    k.contains(p)
                }
            }) {
                matched = Some(g.heading);
                break;
            }
        }
        if let Some(heading) = matched {
            by_group.entry(heading).or_default().push((*k, *v));
        } else {
            leftovers.push((*k, *v));
        }
    }

    let breakdown_open = state.focused_stat.is_some();

    egui::SidePanel::right("calcs_breakdown_panel")
        .resizable(true)
        .default_width(420.0)
        .min_width(320.0)
        .show_animated_inside(ui, breakdown_open, |ui| {
            if let Some(focus) = state.focused_stat.clone() {
                render_focused_breakdown(ui, env, &focus);
            }
        });

    egui::ScrollArea::vertical()
        .id_salt("calcs_list")
        .auto_shrink([false, false])
        .show(ui, |ui| {
            // Three-column flow mirroring PoB's CalcSections layout. Each
            // group declares its column in `GROUPS.column`; `ui.columns`
            // divides the available width evenly so the tab fills the
            // panel instead of stacking into one narrow strip.
            ui.columns(3, |cols| {
                for (col_idx, col_ui) in cols.iter_mut().enumerate() {
                    render_flat_column(col_ui, col_idx as u8, &by_group, &mut state.focused_stat);
                }
            });
            if !leftovers.is_empty() {
                ui.separator();
                ui.collapsing("Other", |ui| {
                    egui::Grid::new("calcs_grid_other")
                        .num_columns(2)
                        .striped(true)
                        .show(ui, |ui| {
                            for (k, v) in &leftovers {
                                render_row(ui, k, *v, &mut state.focused_stat);
                            }
                        });
                });
            }
        });
    // Issue #207 follow-up: flat-layout tail — same LRU update as the
    // PoB-layout early-return path.
    update_recents_lru(state, prev_focused.as_deref());
}

/// Issue #207 follow-up: if the focused-stat selection changed during
/// this frame, push the new value into the recents LRU. Pulled out so
/// both layout paths in [`ui`] share one tail.
fn update_recents_lru(state: &mut CalcsTabState, prev_focused: Option<&str>) {
    let now = state.focused_stat.as_deref();
    if now != prev_focused {
        if let Some(stat) = now {
            push_recent_stat(&mut state.recently_focused, stat, RECENTLY_FOCUSED_MAX);
        }
    }
}

fn render_flat_column(
    ui: &mut egui::Ui,
    column: u8,
    by_group: &std::collections::HashMap<&str, Vec<(&str, f64)>>,
    focused: &mut Option<String>,
) {
    ui.label(
        egui::RichText::new(group_heading(column as usize))
            .strong()
            .underline(),
    );
    ui.add_space(2.0);
    for g in GROUPS.iter().filter(|g| g.column == column) {
        let Some(rows) = by_group.get(g.heading) else {
            continue;
        };
        if rows.is_empty() {
            continue;
        }
        egui::CollapsingHeader::new(g.heading)
            .id_salt(("flat_calc_group", g.heading))
            .default_open(true)
            .show(ui, |ui| {
                egui::Grid::new(format!("calcs_grid_{}", g.heading))
                    .num_columns(2)
                    .striped(true)
                    .show(ui, |ui| {
                        for (k, v) in rows {
                            render_row(ui, k, *v, focused);
                        }
                    });
            });
    }
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

    let breakdown_open = state.focused_stat.is_some();

    egui::SidePanel::right("calcs_pob_breakdown_panel")
        .resizable(true)
        .default_width(420.0)
        .min_width(320.0)
        .show_animated_inside(ui, breakdown_open, |ui| {
            if let Some(focus) = state.focused_stat.clone() {
                render_focused_breakdown(ui, env, &focus);
            }
        });

    egui::ScrollArea::vertical()
        .id_salt("calcs_pob_layout")
        .auto_shrink([false, false])
        .show(ui, |ui| {
            ui.columns(3, |cols| {
                for (col_idx, col_ui) in cols.iter_mut().enumerate() {
                    col_ui.label(
                        egui::RichText::new(group_heading(col_idx))
                            .strong()
                            .underline(),
                    );
                    col_ui.add_space(2.0);
                    for section in &by_group[col_idx] {
                        render_section(
                            col_ui,
                            section,
                            output,
                            &q,
                            state.hide_zero,
                            active_skill_flags,
                            &mut state.focused_stat,
                        );
                    }
                }
            });
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

/// Build the hover-tooltip lines for a Calcs-tab output row.
///
/// Why a pure formatter: keeps the description table testable and lets
/// the tree of egui calls in `render_calc_row` / `render_row` stay a
/// thin wiring layer.
///
/// First line is the user-visible label (or the output key when no
/// label is given). Where the key differs from the label we surface
/// `Output key: <key>` so the user can correlate the row with the raw
/// names that appear in calc_breakdown / engine logs. Known keys also
/// get a one-line plain-English description from
/// [`describe_output_key`]; unknown keys still get the key + click hint.
pub fn calc_row_tooltip_lines(label: &str, key: Option<&str>) -> Vec<String> {
    let mut lines = Vec::new();
    let header = if !label.is_empty() {
        label.to_owned()
    } else {
        key.unwrap_or("(unnamed)").to_owned()
    };
    lines.push(header.clone());
    if let Some(k) = key {
        if k != header {
            lines.push(format!("Output key: {k}"));
        }
        if let Some(desc) = describe_output_key(k) {
            lines.push(desc.to_owned());
        }
        lines.push("Click to see contributing modifiers".to_owned());
    }
    lines
}

/// One-line plain-English summary for an engine output key. Returns
/// `None` for keys we haven't curated yet — the caller still surfaces
/// the raw key, so this table grows opportunistically rather than
/// trying to mirror every output up-front.
fn describe_output_key(key: &str) -> Option<&'static str> {
    Some(match key {
        "MainSkillDPS" => "Main skill damage per second after all modifiers.",
        "FullDPS" => "Combined damage per second across the main skill and all secondary sources (ailments, minions, totems).",
        "TotalDPS" => "Total damage per second of the main skill before ailment / secondary stacking.",
        "MainSkillAverageHit" => "Average damage of a single hit of the main skill, before resistances and mitigation.",
        "MainSkillAverageHitAfterResist" => "Average per-hit damage after the enemy's resistances are applied.",
        "MainSkillAverageHitAfterShock" => "Average per-hit damage after the enemy's shock effect amplifies it.",
        "MainSkillAverageHitAfterAccuracy" => "Average per-hit damage after multiplying by hit chance.",
        "MainSkillSpeed" => "Hits per second of the main skill (attack speed × hits per attack).",
        "MainSkillHitChance" => "Probability that an attack lands on the configured enemy (0–100%).",
        "MainSkillManaCost" => "Mana cost per use of the main skill, after cost / efficiency modifiers.",
        "ManaPerSecondCost" => "Sustained mana drain of the main skill (cost × uses per second).",
        "WithBleedDPS" => "Combined per-target DPS of the main hit plus bleeding stacks it inflicts.",
        "WithPoisonDPS" => "Combined per-target DPS of the main hit plus poison stacks it inflicts.",
        "WithIgniteDPS" => "Combined per-target DPS of the main hit plus the ignite it inflicts.",
        "WithImpaleDPS" => "Combined per-target DPS of the main hit plus the impale stacks it inflicts.",
        "BleedDPS" => "DPS of the bleed ailment alone (single application, scaled by hit and ailment modifiers).",
        "PoisonDPS" => "DPS of the poison ailment alone, summed over the active stack count.",
        "IgniteDPS" => "DPS of the ignite alone (single ignite, scaled by hit and ailment modifiers).",
        "ImpaleDPS" => "DPS contribution from impale stacks (stored damage × stacks × proc chance × hits/sec).",
        "TotalEHP" => "Effective HP — how much raw enemy damage you can take before dying, averaged over hit types.",
        "AverageEHP" => "Mean effective HP across the configured damage-type distribution.",
        "MinimumEHP" => "Worst-case effective HP — the damage type the build is least defended against.",
        "PhysicalEHP" => "Effective HP against physical hits, after armour, evasion, block, and pools.",
        "FireEHP" => "Effective HP against fire hits, after resistance and pool mitigation.",
        "ColdEHP" => "Effective HP against cold hits, after resistance and pool mitigation.",
        "LightningEHP" => "Effective HP against lightning hits, after resistance and pool mitigation.",
        "ChaosEHP" => "Effective HP against chaos hits (typically bypasses ES unless explicitly mitigated).",
        "PhysicalDamageReduction" => "Percentage of incoming physical damage cancelled by armour, endurance charges, and similar.",
        "NumberOfDamagingHits" => "Damaging hits the configured enemy needs to land to deplete the configured pool.",
        "EHPSurvivalTime" => "Seconds the build survives a continuous stream of the configured enemy hits.",
        "Str" => "Strength stat after all modifiers — drives life, melee phys, and Str-scaled mods.",
        "Dex" => "Dexterity stat after all modifiers — drives accuracy, evasion, and Dex-scaled mods.",
        "Int" => "Intelligence stat after all modifiers — drives mana, ES, and Int-scaled mods.",
        "Accuracy" => "Total accuracy rating — feeds into MainSkillHitChance against the configured enemy.",
        _ => return None,
    })
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
        let lines = calc_row_tooltip_lines(&label_text, row.output_key.as_deref());
        label.on_hover_ui(|ui| {
            for line in &lines {
                ui.label(line);
            }
        });
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

/// Issue #34 follow-up: serialise the entire `Output` dictionary as a
/// plain-text dump for clipboard export. Keys sort alphabetically so
/// the output is deterministic across runs (the engine's HashMap
/// iteration order isn't). Numbers go through [`format_value`] so
/// the export matches what the on-screen flat-list view shows.
///
/// Pure / no egui — the call site copies the returned string into the
/// clipboard. Empty outputs produce an empty string (the renderer
/// disables the button in that case but the helper handles it cleanly
/// regardless).
#[must_use]
pub fn format_output_as_text(output: &Output) -> String {
    let mut entries: Vec<(&str, f64)> = output.iter().collect();
    entries.sort_by(|a, b| a.0.cmp(b.0));
    let mut out = String::new();
    for (k, v) in entries {
        out.push_str(k);
        out.push_str(": ");
        out.push_str(format_value(v).trim());
        out.push('\n');
    }
    out
}

/// Issue #34 follow-up: filtered variant of [`format_output_as_text`]
/// that mirrors what the user sees on screen. Applies the same
/// case-insensitive substring filter and zero-suppression rule as
/// the flat-view renderer (see [`count_filtered_output_entries`]) so
/// the clipboard paste matches the visible table row-for-row.
///
/// Empty / whitespace-only `filter` is a "no filter" pass-through.
/// `hide_zero = false` keeps every key regardless of value. Both
/// filters off is equivalent to [`format_output_as_text`].
#[must_use]
pub fn format_output_filtered_as_text(output: &Output, filter: &str, hide_zero: bool) -> String {
    let q = filter.trim().to_ascii_lowercase();
    let mut entries: Vec<(&str, f64)> = output
        .iter()
        .filter(|(k, _)| q.is_empty() || k.to_ascii_lowercase().contains(&q))
        .filter(|(_, v)| !hide_zero || v.abs() > 1e-9)
        .collect();
    entries.sort_by(|a, b| a.0.cmp(b.0));
    let mut out = String::new();
    for (k, v) in entries {
        out.push_str(k);
        out.push_str(": ");
        out.push_str(format_value(v).trim());
        out.push('\n');
    }
    out
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
        let lines = calc_row_tooltip_lines(k, Some(k));
        label.on_hover_ui(|ui| {
            for line in &lines {
                ui.label(line);
            }
        });
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

/// Issue #203 (slice 5): build the hover-tooltip lines for a single
/// `Mod` row in the Calcs side panel — the user wants the parsed
/// structured form (key, type, tags, flags) for debugging
/// mod_parser fall-throughs and verifying source attribution.
///
/// Pure formatter so the egui hover-ui call site stays a thin
/// wiring layer. Lines composed:
///
/// - `<stat-key> · <KIND>` header (always)
/// - `Value: <fmt>` (always — Number / Range / Bool / Str)
/// - `Flags: A | B | …` (only when `m.flags` is non-empty)
/// - `Keyword flags: A | B | …` (only when `m.keyword_flags` is non-empty)
/// - `Source: <label>` (always — humanised by `source_label`)
/// - `Tag: <fmt>` per tag (only when `m.tags` is non-empty)
pub fn mod_tooltip_lines(m: &Mod) -> Vec<String> {
    let mut lines = Vec::new();
    lines.push(format!("{} · {}", m.name, kind_label(m.kind)));
    let value = match &m.value {
        pob_engine::ModValue::Number(n) => format!("{n:+.2}"),
        pob_engine::ModValue::Range { min, max } => format!("{min:.0}-{max:.0} (Range)"),
        pob_engine::ModValue::Bool(b) => b.to_string(),
        pob_engine::ModValue::Str(s) => format!("\"{s}\""),
    };
    lines.push(format!("Value: {value}"));

    if !m.flags.is_empty() {
        let mut flag_names = Vec::new();
        for (name, bit) in &[
            ("ATTACK", pob_data::ModFlag::ATTACK),
            ("SPELL", pob_data::ModFlag::SPELL),
            ("MELEE", pob_data::ModFlag::MELEE),
            ("PROJECTILE", pob_data::ModFlag::PROJECTILE),
            ("AREA", pob_data::ModFlag::AREA),
            ("BOW", pob_data::ModFlag::BOW),
            ("CLAW", pob_data::ModFlag::CLAW),
            ("DAGGER", pob_data::ModFlag::DAGGER),
            ("MACE", pob_data::ModFlag::MACE),
            ("STAFF", pob_data::ModFlag::STAFF),
            ("SWORD", pob_data::ModFlag::SWORD),
            ("WAND", pob_data::ModFlag::WAND),
            ("AXE", pob_data::ModFlag::AXE),
            ("WEAPON_1H", pob_data::ModFlag::WEAPON_1H),
            ("WEAPON_2H", pob_data::ModFlag::WEAPON_2H),
        ] {
            if m.flags.contains(*bit) {
                flag_names.push(*name);
            }
        }
        if !flag_names.is_empty() {
            lines.push(format!("Flags: {}", flag_names.join(" | ")));
        }
    }

    if !m.keyword_flags.is_empty() {
        let mut kw_names = Vec::new();
        for (name, bit) in &[
            ("PHYSICAL", pob_data::KeywordFlag::PHYSICAL),
            ("FIRE", pob_data::KeywordFlag::FIRE),
            ("COLD", pob_data::KeywordFlag::COLD),
            ("LIGHTNING", pob_data::KeywordFlag::LIGHTNING),
            ("CHAOS", pob_data::KeywordFlag::CHAOS),
            ("AILMENT", pob_data::KeywordFlag::AILMENT),
        ] {
            if m.keyword_flags.contains(*bit) {
                kw_names.push(*name);
            }
        }
        if !kw_names.is_empty() {
            lines.push(format!("Keyword flags: {}", kw_names.join(" | ")));
        }
    }

    lines.push(format!("Source: {}", source_label(m.source.as_ref())));
    for t in &m.tags {
        lines.push(format!("Tag: {}", format_tag(t)));
    }
    lines
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
    let label = ui.label(text);
    if label.hovered() {
        let lines = mod_tooltip_lines(m);
        label.on_hover_ui(|ui| {
            for line in &lines {
                ui.label(line);
            }
        });
    }
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
    use super::{
        calc_row_tooltip_lines, count_filtered_output_entries, format_output_as_text,
        format_output_filtered_as_text, mod_tooltip_lines, push_recent_stat, reset_calcs_view,
        sort_flat_entries, CalcsFlatSort, CalcsTabState, GROUPS, RECENTLY_FOCUSED_MAX,
    };
    use pob_engine::{Mod, Output};
    use std::collections::VecDeque;

    // ─── count_filtered_output_entries ───────────────────────────────────

    fn make_output(pairs: &[(&str, f64)]) -> Output {
        let mut o = Output::default();
        for (k, v) in pairs {
            o.set(*k, *v);
        }
        o
    }

    #[test]
    fn count_filtered_output_no_filter_returns_full_count() {
        // The cold-open path: no substring filter, hide_zero off.
        // Helper must match the raw `Output::len`.
        let out = make_output(&[("Life", 5000.0), ("Mana", 0.0), ("FireResist", 75.0)]);
        assert_eq!(count_filtered_output_entries(&out, "", false), 3);
    }

    #[test]
    fn count_filtered_output_hide_zero_drops_zero_valued_entries() {
        // `hide_zero` skips entries whose value is effectively zero
        // (`abs(v) < 1e-9`) so a polled output dictionary with
        // unfilled stats doesn't waste a row in the table.
        let out = make_output(&[("Life", 5000.0), ("Mana", 0.0), ("ColdResist", 0.0)]);
        assert_eq!(count_filtered_output_entries(&out, "", true), 1);
    }

    #[test]
    fn count_filtered_output_substring_filter_is_case_insensitive() {
        // Mirrors the inline filter rule in the flat-view renderer —
        // case-insensitive substring against the key.
        let out = make_output(&[("FireResist", 75.0), ("ColdResist", 75.0), ("Life", 5000.0)]);
        assert_eq!(count_filtered_output_entries(&out, "resist", false), 2);
        assert_eq!(count_filtered_output_entries(&out, "RESIST", false), 2);
        assert_eq!(count_filtered_output_entries(&out, "Life", false), 1);
    }

    #[test]
    fn count_filtered_output_filter_and_hide_zero_compose() {
        // Both filters active simultaneously: keep only resists with
        // non-zero values.
        let out = make_output(&[
            ("FireResist", 75.0),
            ("ColdResist", 0.0),
            ("LightningResist", 30.0),
        ]);
        assert_eq!(count_filtered_output_entries(&out, "resist", true), 2);
    }

    #[test]
    fn count_filtered_output_whitespace_filter_is_no_op() {
        // Trim semantics — a whitespace-only buffer must NOT exclude
        // any rows, otherwise the chip flickers as the user types and
        // backspaces.
        let out = make_output(&[("Life", 5000.0), ("Mana", 100.0)]);
        assert_eq!(count_filtered_output_entries(&out, "   ", false), 2);
    }

    #[test]
    fn reset_calcs_view_clears_inflight_ui_state() {
        // Build a CalcsTabState with every field exercised, then
        // confirm `reset_calcs_view` zeroes the in-flight pieces.
        let mut state = CalcsTabState::default();
        state.filter = "FireResist".to_owned();
        state.hide_zero = true;
        state.focused_stat = Some("Life".to_owned());
        state.use_pob_layout = true;
        state.flat_sort = CalcsFlatSort::ValueDesc;
        state.recently_focused.push_back("Life".into());
        state.recently_focused.push_back("Mana".into());

        reset_calcs_view(&mut state);

        assert!(state.filter.is_empty(), "filter should be cleared");
        assert!(state.focused_stat.is_none(), "breakdown focus should close");
        assert!(state.recently_focused.is_empty(), "recents row should drop");
        assert_eq!(
            state.flat_sort,
            CalcsFlatSort::default(),
            "flat sort should return to default"
        );
    }

    #[test]
    fn reset_calcs_view_preserves_preferences() {
        // `hide_zero` and `use_pob_layout` are deliberate user
        // preferences — they should not be wiped by a view reset.
        let mut state = CalcsTabState::default();
        state.hide_zero = true;
        state.use_pob_layout = true;

        reset_calcs_view(&mut state);

        assert!(state.hide_zero, "hide-zero preference should persist");
        assert!(state.use_pob_layout, "pob-layout preference should persist");
    }

    #[test]
    fn sort_flat_entries_key_mode_is_alphabetical() {
        // Default mode — matches the pre-this-slice ordering so
        // existing users see no behavioural drift.
        let mut entries: Vec<(&str, f64)> =
            vec![("Mana", 200.0), ("Damage", 1500.0), ("Life", 5000.0)];
        sort_flat_entries(&mut entries, CalcsFlatSort::Key);
        let keys: Vec<&str> = entries.iter().map(|(k, _)| *k).collect();
        assert_eq!(keys, vec!["Damage", "Life", "Mana"]);
    }

    #[test]
    fn sort_flat_entries_value_desc_largest_first() {
        let mut entries: Vec<(&str, f64)> =
            vec![("Mana", 200.0), ("Damage", 1500.0), ("Life", 5000.0)];
        sort_flat_entries(&mut entries, CalcsFlatSort::ValueDesc);
        let keys: Vec<&str> = entries.iter().map(|(k, _)| *k).collect();
        assert_eq!(keys, vec!["Life", "Damage", "Mana"]);
    }

    #[test]
    fn sort_flat_entries_value_asc_smallest_first() {
        let mut entries: Vec<(&str, f64)> =
            vec![("Mana", 200.0), ("Damage", 1500.0), ("Life", 5000.0)];
        sort_flat_entries(&mut entries, CalcsFlatSort::ValueAsc);
        let keys: Vec<&str> = entries.iter().map(|(k, _)| *k).collect();
        assert_eq!(keys, vec!["Mana", "Damage", "Life"]);
    }

    #[test]
    fn sort_flat_entries_value_modes_break_ties_alphabetically() {
        // When two entries share a value, the secondary key is the
        // output key so the order stays deterministic.
        let mut entries: Vec<(&str, f64)> = vec![("Zeta", 100.0), ("Alpha", 100.0), ("Mu", 100.0)];
        sort_flat_entries(&mut entries, CalcsFlatSort::ValueDesc);
        let keys: Vec<&str> = entries.iter().map(|(k, _)| *k).collect();
        assert_eq!(keys, vec!["Alpha", "Mu", "Zeta"]);
    }

    #[test]
    fn sort_flat_entries_value_desc_sinks_nan() {
        // A stray engine NaN must not pin garbage to the top; NaN
        // entries sort to the bottom regardless of their position
        // in the input.
        let mut entries: Vec<(&str, f64)> =
            vec![("Real", 50.0), ("Broken", f64::NAN), ("Bigger", 500.0)];
        sort_flat_entries(&mut entries, CalcsFlatSort::ValueDesc);
        let keys: Vec<&str> = entries.iter().map(|(k, _)| *k).collect();
        assert_eq!(keys, vec!["Bigger", "Real", "Broken"]);
    }

    fn out_with(pairs: &[(&str, f64)]) -> Output {
        let mut o = Output::default();
        for (k, v) in pairs {
            o.set(*k, *v);
        }
        o
    }

    #[test]
    fn format_output_as_text_emits_alphabetical_keyvalue_lines() {
        // Engine HashMap iteration order isn't stable; the formatter
        // sorts alphabetically so the export is reproducible.
        let out = out_with(&[
            ("MainSkillDPS", 1500.0),
            ("FireResist", 75.0),
            ("Life", 5000.0),
        ]);
        let text = format_output_as_text(&out);
        let lines: Vec<&str> = text.lines().collect();
        assert_eq!(lines.len(), 3);
        assert_eq!(lines[0], "FireResist: 75");
        assert_eq!(lines[1], "Life: 5000");
        assert_eq!(lines[2], "MainSkillDPS: 1500");
    }

    #[test]
    fn format_output_as_text_empty_input_returns_empty_string() {
        let out = Output::default();
        assert!(format_output_as_text(&out).is_empty());
    }

    #[test]
    fn format_output_as_text_uses_format_value_for_decimals() {
        // Pin the formatter's behaviour: integers stay integer-shaped,
        // small-magnitude fractions get the high-precision form.
        // format_value uses 4 decimal places for small fractions, 2
        // for larger ones, integer form when the fract is zero.
        let out = out_with(&[
            ("CritChance", 5.25),
            ("HitChance", 100.0),
            ("Damage", 1500.5),
        ]);
        let text = format_output_as_text(&out);
        let lines: Vec<&str> = text.lines().collect();
        assert_eq!(lines[0], "CritChance: 5.2500");
        assert_eq!(lines[1], "Damage: 1500.50");
        assert_eq!(lines[2], "HitChance: 100");
    }

    // ─── format_output_filtered_as_text ──────────────────────────────────

    #[test]
    fn format_output_filtered_no_filter_matches_unfiltered_form() {
        // Cold-open contract: with no filter and hide_zero off, the
        // filtered helper must produce byte-for-byte the same output
        // as the existing unfiltered formatter so a no-filter Copy
        // still reads identically.
        let out = out_with(&[
            ("CritChance", 5.25),
            ("HitChance", 100.0),
            ("Damage", 1500.5),
        ]);
        assert_eq!(
            format_output_filtered_as_text(&out, "", false),
            format_output_as_text(&out),
        );
    }

    #[test]
    fn format_output_filtered_substring_filter_keeps_matching_keys_only() {
        // Filter narrowing should mirror the on-screen table — only
        // matching keys land in the clipboard.
        let out = out_with(&[("FireResist", 75.0), ("ColdResist", 75.0), ("Life", 5000.0)]);
        let text = format_output_filtered_as_text(&out, "resist", false);
        let lines: Vec<&str> = text.lines().collect();
        assert_eq!(lines.len(), 2);
        assert!(lines.iter().all(|l| l.contains("Resist")));
        assert!(!text.contains("Life"));
    }

    #[test]
    fn format_output_filtered_hide_zero_drops_zero_valued_keys() {
        // Mirrors the renderer's hide_zero rule. Defends against a
        // user whose paste used to be cluttered with zero-valued
        // placeholder outputs after the engine populated them.
        let out = out_with(&[("Life", 5000.0), ("Mana", 0.0), ("ColdResist", 0.0)]);
        let text = format_output_filtered_as_text(&out, "", true);
        let lines: Vec<&str> = text.lines().collect();
        assert_eq!(lines.len(), 1);
        assert!(lines[0].starts_with("Life: "));
    }

    #[test]
    fn format_output_filtered_filter_and_hide_zero_compose() {
        // Both filters active simultaneously: keep only resists with
        // non-zero values.
        let out = out_with(&[
            ("FireResist", 75.0),
            ("ColdResist", 0.0),
            ("LightningResist", 30.0),
        ]);
        let text = format_output_filtered_as_text(&out, "resist", true);
        let lines: Vec<&str> = text.lines().collect();
        assert_eq!(lines.len(), 2);
        assert!(lines[0].starts_with("FireResist: "));
        assert!(lines[1].starts_with("LightningResist: "));
    }

    #[test]
    fn format_output_filtered_whitespace_filter_is_pass_through() {
        // Trim semantics keep parity with `count_filtered_output_entries`
        // — a whitespace-only buffer must NOT exclude any keys.
        let out = out_with(&[("Life", 5000.0), ("Mana", 100.0)]);
        let text = format_output_filtered_as_text(&out, "   ", false);
        assert_eq!(text.lines().count(), 2);
    }

    #[test]
    fn push_recent_stat_inserts_at_front_when_new() {
        let mut q: VecDeque<String> = VecDeque::new();
        push_recent_stat(&mut q, "Life", RECENTLY_FOCUSED_MAX);
        assert_eq!(q.front().map(String::as_str), Some("Life"));
        push_recent_stat(&mut q, "Mana", RECENTLY_FOCUSED_MAX);
        // Most recent at the front; previous entry shifts back.
        assert_eq!(
            q.iter().map(String::as_str).collect::<Vec<_>>(),
            vec!["Mana", "Life"],
        );
    }

    #[test]
    fn push_recent_stat_dedups_and_promotes_to_front() {
        // Revisiting a stat moves it back to position 0 rather than
        // creating a duplicate. Lets the chip row read as "most recent
        // 5 unique stats".
        let mut q: VecDeque<String> = VecDeque::new();
        push_recent_stat(&mut q, "Life", 5);
        push_recent_stat(&mut q, "Mana", 5);
        push_recent_stat(&mut q, "Life", 5);
        assert_eq!(
            q.iter().map(String::as_str).collect::<Vec<_>>(),
            vec!["Life", "Mana"],
        );
        assert_eq!(q.len(), 2, "no duplicate Life entry");
    }

    #[test]
    fn push_recent_stat_truncates_to_max_len() {
        // Inserting past the cap drops the oldest entry off the tail.
        let mut q: VecDeque<String> = VecDeque::new();
        for name in ["a", "b", "c", "d", "e", "f"] {
            push_recent_stat(&mut q, name, 5);
        }
        // f, e, d, c, b — a fell off the back.
        assert_eq!(
            q.iter().map(String::as_str).collect::<Vec<_>>(),
            vec!["f", "e", "d", "c", "b"],
        );
    }

    #[test]
    fn push_recent_stat_drops_empty_input() {
        // Defensive: the breakdown panel can briefly hold an empty
        // focused_stat during a state transition; the helper should
        // not record that.
        let mut q: VecDeque<String> = VecDeque::new();
        push_recent_stat(&mut q, "", 5);
        assert!(q.is_empty());
    }

    #[test]
    fn push_recent_stat_respects_explicit_zero_cap() {
        // `max_len = 0` is a degenerate config but should still
        // produce a clean result (immediately truncate back to empty).
        let mut q: VecDeque<String> = VecDeque::new();
        push_recent_stat(&mut q, "Life", 0);
        assert!(q.is_empty());
    }

    #[test]
    fn mod_tooltip_includes_stat_key_and_kind_label() {
        let m = Mod::inc("MainSkillDPS", 25.0);
        let lines = mod_tooltip_lines(&m);
        assert!(
            lines.iter().any(|l| l.contains("MainSkillDPS")),
            "expected stat key in tooltip, got {lines:?}"
        );
        assert!(
            lines.iter().any(|l| l.contains("INC")),
            "expected INC kind label in tooltip, got {lines:?}"
        );
    }

    #[test]
    fn mod_tooltip_omits_flag_lines_when_no_flags() {
        let m = Mod::base("PhysicalDamage", 10.0);
        let lines = mod_tooltip_lines(&m);
        assert!(
            !lines.iter().any(|l| l.starts_with("Flags:")),
            "expected no Flags line for unflagged mod, got {lines:?}"
        );
        assert!(
            !lines.iter().any(|l| l.starts_with("Keyword flags:")),
            "expected no Keyword flags line for unflagged mod, got {lines:?}"
        );
    }

    #[test]
    fn mod_tooltip_lists_modflag_and_keyword_flag() {
        let mut m = Mod::inc("PhysicalDamage", 10.0);
        m.flags |= pob_data::ModFlag::CLAW;
        m.keyword_flags |= pob_data::KeywordFlag::PHYSICAL;
        let lines = mod_tooltip_lines(&m);
        assert!(
            lines
                .iter()
                .any(|l| l.starts_with("Flags:") && l.contains("CLAW")),
            "expected CLAW in Flags line, got {lines:?}"
        );
        assert!(
            lines
                .iter()
                .any(|l| l.starts_with("Keyword flags:") && l.contains("PHYSICAL")),
            "expected PHYSICAL in Keyword flags line, got {lines:?}"
        );
    }

    #[test]
    fn mod_tooltip_includes_value_for_range() {
        let m = Mod::base(
            "LightningDamage",
            pob_engine::ModValue::Range {
                min: 0.0,
                max: 25.0,
            },
        );
        let lines = mod_tooltip_lines(&m);
        assert!(
            lines
                .iter()
                .any(|l| l.contains("0-25") || l.contains("Range")),
            "expected range value in tooltip, got {lines:?}"
        );
    }

    #[test]
    fn tooltip_header_uses_label_when_present() {
        let lines = calc_row_tooltip_lines("Total DPS", Some("MainSkillDPS"));
        assert_eq!(lines.first().map(String::as_str), Some("Total DPS"));
    }

    #[test]
    fn tooltip_header_falls_back_to_key_when_label_empty() {
        let lines = calc_row_tooltip_lines("", Some("MainSkillDPS"));
        assert_eq!(lines.first().map(String::as_str), Some("MainSkillDPS"));
    }

    #[test]
    fn tooltip_includes_output_key_line_when_label_differs() {
        let lines = calc_row_tooltip_lines("Total DPS", Some("MainSkillDPS"));
        assert!(
            lines.iter().any(|l| l == "Output key: MainSkillDPS"),
            "expected output-key disclosure, got {lines:?}"
        );
    }

    #[test]
    fn tooltip_omits_output_key_line_when_label_matches_key() {
        let lines = calc_row_tooltip_lines("MainSkillDPS", Some("MainSkillDPS"));
        assert!(
            !lines.iter().any(|l| l.starts_with("Output key:")),
            "expected no redundant output-key line, got {lines:?}"
        );
    }

    #[test]
    fn tooltip_adds_known_description_for_main_skill_dps() {
        let lines = calc_row_tooltip_lines("Total DPS", Some("MainSkillDPS"));
        assert!(
            lines.iter().any(|l| l.contains("damage per second")),
            "expected MainSkillDPS description, got {lines:?}"
        );
    }

    #[test]
    fn tooltip_adds_click_hint_when_key_is_known() {
        let lines = calc_row_tooltip_lines("Total DPS", Some("MainSkillDPS"));
        assert_eq!(
            lines.last().map(String::as_str),
            Some("Click to see contributing modifiers")
        );
    }

    #[test]
    fn tooltip_omits_click_hint_when_no_key() {
        let lines = calc_row_tooltip_lines("Section heading", None);
        assert!(
            !lines.iter().any(|l| l.starts_with("Click to see")),
            "expected no click hint without an output key, got {lines:?}"
        );
    }

    #[test]
    fn tooltip_unknown_key_still_yields_header_and_hint() {
        let lines = calc_row_tooltip_lines("Some Random", Some("CompletelyUnknownOutputKeyXyz"));
        assert_eq!(lines.first().map(String::as_str), Some("Some Random"));
        assert!(lines
            .iter()
            .any(|l| l == "Output key: CompletelyUnknownOutputKeyXyz"));
        assert_eq!(
            lines.last().map(String::as_str),
            Some("Click to see contributing modifiers")
        );
    }

    /// Returns the first group heading whose patterns match `key`, or
    /// `None` if it falls through to "Other". Matches the same shape
    /// as the runtime grouping code (substring match, with `:` suffix
    /// handled as a strict prefix).
    fn group_for(key: &str) -> Option<&'static str> {
        for g in GROUPS {
            for p in g.patterns {
                let hit = if p.ends_with(':') {
                    key.starts_with(p)
                } else {
                    key.contains(p)
                };
                if hit {
                    return Some(g.heading);
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
            // Issue #145 (slice 5): Rallying Cry ally weapon-class
            // projection back to the player. `Projected` is the count
            // of (ally × weapon-class) pairs the cry generated mods
            // for; `Matched` is the total MORE Damage% the player's
            // currently-wielded weapon class actually picks up.
            "RallyingCryAllyWeaponClassesProjected",
            "RallyingCryAllyWeaponDamageMatched",
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
            // Slice 16: crit factor + total HP pool.
            "MinionCritFactor",
            "MinionTotalHP",
            // Slice 11: life regen rate.
            "MinionLifeRegenPercent",
            "MinionLifeRegen",
            // Slice 13: energy shield base + scaled output.
            "MinionEnergyShieldBase",
            "MinionEnergyShield",
            // Slice 14: armour and evasion base + scaled output.
            "MinionArmourBase",
            "MinionArmour",
            "MinionEvasionBase",
            "MinionEvasion",
            // Slice 15: movement speed multiplier + percentage output.
            "MinionMovementSpeedMod",
            "MinionMovementSpeed",
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
