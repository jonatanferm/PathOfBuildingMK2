//! The Skills tab, driven by Path of Building's own Skills tab: its socket
//! group list (PoB's labels, slots, Full DPS flags) and, for the group
//! being edited, PoB's own group and gem-slot controls — so gem selection,
//! level limits, supports and item-granted groups behave exactly as in PoB.

#![cfg(not(target_arch = "wasm32"))]

use serde_json::Value;

use crate::pob_controls::{self, Ctl, EditBufs};
use crate::pob_session::{lua_str, PobSession};
use crate::pob_tooltip::{self, Fetcher};

#[derive(Debug, Clone, Default, serde::Deserialize)]
struct Group {
    #[serde(default)]
    label: String,
    #[serde(default)]
    enabled: bool,
    #[serde(default)]
    slot: String,
    #[serde(default)]
    full: bool,
    #[serde(default)]
    source: bool,
    #[serde(default)]
    n: usize,
}

#[derive(Debug, Clone, serde::Deserialize)]
struct GemRow {
    #[serde(default)]
    name: String,
    #[serde(default)]
    color: String,
    #[serde(default)]
    err: String,
    level: Ctl,
    quality: Ctl,
    enabled: Ctl,
    count: Ctl,
    g1: Ctl,
    g2: Ctl,
}

#[derive(Debug, Clone, Default, serde::Deserialize)]
struct Skills {
    #[serde(default)]
    groups: Vec<Group>,
    #[serde(default)]
    display: usize,
    #[serde(default)]
    main: usize,
    #[serde(default)]
    ctls: Vec<Ctl>,
    #[serde(default, rename = "fromItem")]
    from_item: bool,
    #[serde(default)]
    gems: Vec<GemRow>,
}

#[derive(Debug, Clone, serde::Deserialize)]
struct CatalogGem {
    n: String,
    #[serde(default)]
    s: bool,
    #[serde(default)]
    c: u8,
    #[serde(default)]
    l: bool,
}

#[derive(Default)]
pub struct PobSkillsState {
    skills: Skills,
    pending: Option<u64>,
    fetched_for: Option<u64>,
    catalog: Vec<CatalogGem>,
    catalog_pending: Option<u64>,
    bufs: EditBufs,
    gem_search: String,
    show_legacy: bool,
    tips: Fetcher,
}

fn gem_colour(c: u8) -> egui::Color32 {
    match c {
        1 => egui::Color32::from_rgb(0xE0, 0x50, 0x30),
        2 => egui::Color32::from_rgb(0x70, 0xFF, 0x70),
        3 => egui::Color32::from_rgb(0x70, 0x70, 0xFF),
        _ => egui::Color32::from_gray(200),
    }
}

fn op(pob: &mut PobSession, code: String) {
    pob.command(code);
}

/// Render the tab.
pub fn ui(ui: &mut egui::Ui, state: &mut PobSkillsState, pob: &mut PobSession) {
    let snap_seq = pob.snapshot.as_ref().map(|s| s.seq);
    if state.pending.is_none() && snap_seq.is_some() && state.fetched_for != snap_seq {
        state.pending = Some(pob.query("return __mk2_skills()"));
        state.fetched_for = snap_seq;
    }
    if let Some(seq) = state.pending {
        if let Some(res) = pob.take_query(seq) {
            state.pending = None;
            if let Ok(s) =
                res.and_then(|j| serde_json::from_str::<Skills>(&j).map_err(|e| e.to_string()))
            {
                state.skills = s;
                state.bufs.clear();
            }
        }
    }
    if state.catalog.is_empty() {
        match state.catalog_pending {
            None if snap_seq.is_some() => {
                state.catalog_pending = Some(pob.query("return __mk2_gem_catalog()"));
            }
            Some(seq) => {
                if let Some(res) = pob.take_query(seq) {
                    state.catalog_pending = None;
                    state.catalog = res
                        .ok()
                        .and_then(|j| serde_json::from_str(&j).ok())
                        .unwrap_or_default();
                }
            }
            None => {}
        }
    }
    if state.pending.is_some() || state.catalog_pending.is_some() || state.tips.busy() {
        ui.ctx()
            .request_repaint_after(std::time::Duration::from_millis(30));
    }

    let skills = state.skills.clone();
    ui.horizontal_top(|ui| {
        // ── Socket groups ─────────────────────────────────────────────
        ui.vertical(|ui| {
            ui.set_width(230.0);
            ui.heading("Socket groups");
            ui.horizontal(|ui| {
                if ui.small_button("New").clicked() {
                    op(pob, "__mk2_skills_op('new')\n".to_owned());
                }
                let can_delete = skills
                    .groups
                    .get(skills.display.wrapping_sub(1))
                    .is_some_and(|g| !g.source);
                if ui
                    .add_enabled(can_delete, egui::Button::new("Delete").small())
                    .clicked()
                {
                    op(
                        pob,
                        format!("__mk2_skills_op('delete', {})\n", skills.display),
                    );
                }
            });
            ui.separator();
            egui::ScrollArea::vertical()
                .id_salt("pob_skill_groups")
                .auto_shrink([false, false])
                .max_height(ui.available_height() - 8.0)
                .show(ui, |ui| {
                    for (i, g) in skills.groups.iter().enumerate() {
                        let idx = i + 1;
                        let mut text = String::new();
                        if idx == skills.main {
                            text.push_str("★ ");
                        }
                        text.push_str(if g.label.is_empty() {
                            "<No active skills>"
                        } else {
                            &g.label
                        });
                        let mut rich = egui::RichText::new(text);
                        if !g.enabled {
                            rich = rich.weak();
                        }
                        let resp =
                            ui.selectable_label(idx == skills.display, rich)
                                .on_hover_ui(|ui| {
                                    let key = format!("group:{idx}");
                                    ui.set_max_width(460.0);
                                    match state.tips.get(pob, &key, || {
                                        format!("return __mk2_group_tooltip({idx})")
                                    }) {
                                        Some(lines) => pob_tooltip::render(ui, lines),
                                        None => {
                                            ui.spinner();
                                        }
                                    }
                                });
                        let mut sub = g.slot.clone();
                        if g.full {
                            sub.push_str("  · Full DPS");
                        }
                        if g.source {
                            sub.push_str("  · from item");
                        }
                        if !sub.is_empty() {
                            ui.weak(format!("    {sub}"));
                        }
                        if resp.clicked() && idx != skills.display {
                            // Viewing a group isn't an edit: no recalculation.
                            pob.query_detached(&format!(
                                "__mk2_skills_op('select', {idx}) return ''"
                            ));
                            state.pending = Some(pob.query("return __mk2_skills()"));
                        }
                    }
                });
        });
        ui.separator();

        // ── Selected group ────────────────────────────────────────────
        ui.vertical(|ui| {
            if skills.display == 0 {
                ui.weak("Select a socket group, or create one with New.");
                return;
            }
            let mut ctl_change: Option<(String, Value)> = None;
            egui::Grid::new("pob_group_ctls")
                .num_columns(2)
                .spacing([8.0, 4.0])
                .show(ui, |ui| {
                    for c in skills.ctls.iter().filter(|c| c.shown) {
                        let label = match c.n.as_str() {
                            "groupLabel" => "Label:",
                            "groupSlot" => "Socketed in:",
                            "groupEnabled" => "Enabled:",
                            "includeInFullDPS" => "Include in Full DPS:",
                            "groupCount" => "Count:",
                            _ => "",
                        };
                        ui.label(label);
                        if let Some(v) =
                            pob_controls::show(ui, c, &format!("grp_{}", c.n), &mut state.bufs)
                        {
                            ctl_change = Some((c.n.clone(), v));
                        }
                        ui.end_row();
                    }
                });
            if let Some((name, v)) = ctl_change {
                op(
                    pob,
                    format!(
                        "__mk2_skills_op('ctl', {}, {})\n",
                        lua_str(&name),
                        lua_value(&v)
                    ),
                );
            }
            ui.horizontal(|ui| {
                if skills.display != skills.main && ui.button("Set as main skill").clicked() {
                    op(
                        pob,
                        format!("__mk2_skills_op('main', {})\n", skills.display),
                    );
                }
                if skills.from_item {
                    ui.weak("This group is granted by an item.");
                }
            });
            ui.separator();

            // Gems.
            let mut gem_ops: Vec<String> = Vec::new();
            egui::ScrollArea::vertical()
                .id_salt("pob_gems")
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    // Fixed-width cells so long secondary rows can't push
                    // the numeric columns off-screen.
                    const W: [f32; 6] = [22.0, 186.0, 44.0, 44.0, 52.0, 44.0];
                    let cell = |ui: &mut egui::Ui, w: f32, f: &mut dyn FnMut(&mut egui::Ui)| {
                        ui.allocate_ui_with_layout(
                            egui::vec2(w, 22.0),
                            egui::Layout::left_to_right(egui::Align::Center),
                            |ui| {
                                ui.set_width(w);
                                f(ui);
                            },
                        );
                    };
                    ui.horizontal(|ui| {
                        for (w, h) in W
                            .iter()
                            .zip(["", "Gem", "Level", "Quality", "Enabled", "Count"])
                        {
                            cell(ui, *w, &mut |ui| {
                                ui.strong(h);
                            });
                        }
                    });
                    for (i, gem) in skills.gems.iter().enumerate() {
                        let idx = i + 1;
                        let is_new = idx == skills.gems.len();
                        ui.horizontal(|ui| {
                            cell(ui, W[0], &mut |ui| {
                                if !is_new
                                    && !skills.from_item
                                    && ui
                                        .small_button("x")
                                        .on_hover_text("Remove this gem.")
                                        .clicked()
                                {
                                    gem_ops.push(format!("__mk2_skills_op('gemdel', {idx})\n"));
                                }
                            });
                            cell(ui, W[1], &mut |ui| {
                                if let Some(name) = gem_picker(ui, state, idx, gem, pob) {
                                    gem_ops.push(format!(
                                        "__mk2_skills_op('gem', {idx}, {})\n",
                                        lua_str(&name)
                                    ));
                                }
                            });
                            for (k, c) in [&gem.level, &gem.quality, &gem.enabled, &gem.count]
                                .into_iter()
                                .enumerate()
                            {
                                cell(ui, W[k + 2], &mut |ui| {
                                    if c.shown && !is_new {
                                        if let Some(v) = pob_controls::show(
                                            ui,
                                            c,
                                            &format!("gem{idx}_{}", c.n),
                                            &mut state.bufs,
                                        ) {
                                            gem_ops.push(format!(
                                                "__mk2_skills_op('gemctl', {idx}, {}, {})\n",
                                                lua_str(&c.n),
                                                lua_value(&v)
                                            ));
                                        }
                                    }
                                });
                            }
                        });
                        // Vaal gems: enable the regular / Vaal skill separately.
                        if !is_new && (gem.g1.shown || gem.g2.shown) {
                            ui.horizontal_wrapped(|ui| {
                                ui.add_space(W[0] + 8.0);
                                for c in [&gem.g1, &gem.g2] {
                                    if c.shown {
                                        let mut on = c.state;
                                        let label = c.label.as_deref().unwrap_or("Enabled:");
                                        if ui
                                            .checkbox(&mut on, label.trim_end_matches(':'))
                                            .changed()
                                        {
                                            gem_ops.push(format!(
                                                "__mk2_skills_op('gemctl', {idx}, {}, {on})\n",
                                                lua_str(&c.n)
                                            ));
                                        }
                                    }
                                }
                            });
                        }
                        if !gem.err.is_empty() {
                            ui.horizontal_wrapped(|ui| {
                                ui.add_space(W[0] + 8.0);
                                ui.colored_label(egui::Color32::LIGHT_RED, &gem.err);
                            });
                        }
                    }
                });
            for code in gem_ops {
                op(pob, code);
            }
        });
    });
}

fn lua_value(v: &Value) -> String {
    match v {
        Value::Bool(b) => b.to_string(),
        Value::Number(n) => n.to_string(),
        Value::String(s) => lua_str(s),
        _ => "nil".to_owned(),
    }
}

/// The gem-name cell: a searchable dropdown of PoB's gem list, with PoB's
/// gem tooltip on hover. Returns a newly picked gem name.
fn gem_picker(
    ui: &mut egui::Ui,
    state: &mut PobSkillsState,
    idx: usize,
    gem: &GemRow,
    pob: &mut PobSession,
) -> Option<String> {
    let mut picked = None;
    let colour = crate::pob_sidebar::runs(&format!("{}x", gem.color), egui::Color32::GRAY)
        .first()
        .map_or(egui::Color32::GRAY, |r| r.1);
    let shown = if gem.name.is_empty() {
        egui::RichText::new("+ Add gem…").weak()
    } else {
        egui::RichText::new(&gem.name).color(colour)
    };
    let resp = egui::ComboBox::from_id_salt(("pob_gem_pick", idx))
        .selected_text(shown)
        .width(180.0)
        .height(360.0)
        .truncate()
        .show_ui(ui, |ui| {
            let r = ui.add(
                egui::TextEdit::singleline(&mut state.gem_search)
                    .hint_text("Search gems…")
                    .desired_width(240.0),
            );
            r.request_focus();
            ui.checkbox(&mut state.show_legacy, "Show legacy gems");
            let search = state.gem_search.to_lowercase();
            let mut shown = 0;
            for g in &state.catalog {
                if (!state.show_legacy && g.l)
                    || (!search.is_empty() && !g.n.to_lowercase().contains(&search))
                {
                    continue;
                }
                shown += 1;
                if shown > 300 {
                    ui.weak("… refine the search");
                    break;
                }
                let mut text = egui::RichText::new(&g.n).color(gem_colour(g.c));
                if g.s {
                    text = text.italics();
                }
                if ui.selectable_label(g.n == gem.name, text).clicked() {
                    picked = Some(g.n.clone());
                }
            }
        })
        .response;
    if !gem.name.is_empty() {
        resp.on_hover_ui(|ui| {
            ui.set_max_width(480.0);
            let key = format!("gem:{idx}:{}", gem.name);
            match state
                .tips
                .get(pob, &key, || format!("return __mk2_gem_tooltip({idx})"))
            {
                Some(lines) => pob_tooltip::render(ui, lines),
                None => {
                    ui.spinner();
                }
            }
        });
    }
    if picked.is_some() {
        state.gem_search.clear();
    }
    picked.filter(|n| *n != gem.name)
}
