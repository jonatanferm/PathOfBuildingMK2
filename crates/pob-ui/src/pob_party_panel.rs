//! The Party tab, as Path of Building's own Party tab: import party
//! members' auras, curses, warcries, link skills and stats from their
//! build codes, or edit those lists directly. Every control is PoB's.

#![cfg(not(target_arch = "wasm32"))]

use serde_json::Value;

use crate::pob_controls::{self, Ctl, EditBufs};
use crate::pob_session::{lua_str, PobSession};

/// PoB's Party tab controls in display order, grouped into sections.
const LAYOUT: &[(&str, &[&str])] = &[
    ("", &["notesDesc"]),
    (
        "Import",
        &[
            "importCodeHeader",
            "importCodeIn",
            "importCodeState",
            "importCodeDestination",
            "importCodeGo",
            "appendNotReplace",
            "clear",
        ],
    ),
    ("", &["ShowAdvanceTools", "removeEffects", "rebuild"]),
    ("Party member stats", &["editPartyMemberStats"]),
    ("Auras", &["editAuras", "simpleAuras"]),
    ("Warcry skills", &["editWarcries", "simpleWarcries"]),
    ("Link skills", &["editLinks", "simpleLinks"]),
    ("Curses", &["editCurses", "simpleCurses"]),
    ("Enemy conditions", &["enemyCond", "simpleEnemyCond"]),
    ("Enemy modifiers", &["enemyMods", "simpleEnemyMods"]),
];

#[derive(Default)]
pub struct PobPartyState {
    ctls: Vec<Ctl>,
    pending: Option<u64>,
    fetched_for: Option<u64>,
    bufs: EditBufs,
}

fn query() -> String {
    let names: Vec<String> = LAYOUT
        .iter()
        .flat_map(|(_, n)| n.iter())
        .map(|n| lua_str(n))
        .collect();
    format!(
        r#"
local pt = build.partyTab
local out = {{}}
for _, n in ipairs({{ {} }}) do
    local c = pt.controls[n]
    if c then out[#out + 1] = __mk2_ctl(n, c) end
end
return __mk2_json(out)
"#,
        names.join(", ")
    )
}

pub fn ui(ui: &mut egui::Ui, state: &mut PobPartyState, pob: &mut PobSession) {
    let snap = pob.snapshot.as_ref().map(|s| s.seq);
    if state.pending.is_none() && snap.is_some() && state.fetched_for != snap {
        state.pending = Some(pob.query(&query()));
        state.fetched_for = snap;
    }
    if let Some(seq) = state.pending {
        if let Some(res) = pob.take_query(seq) {
            state.pending = None;
            if let Ok(c) = res.and_then(|j| serde_json::from_str(&j).map_err(|e| e.to_string())) {
                state.ctls = c;
                state.bufs.clear();
            }
        } else {
            ui.ctx()
                .request_repaint_after(std::time::Duration::from_millis(30));
        }
    }
    let mut change: Option<(String, Value)> = None;
    egui::ScrollArea::vertical()
        .id_salt("pob_party")
        .auto_shrink([false, false])
        .show(ui, |ui| {
            for (title, names) in LAYOUT {
                // The build-code box sizes itself dynamically in PoB.
                if let Some(c) = state.ctls.iter_mut().find(|c| c.n == "importCodeIn") {
                    c.w = Some(480.0);
                }
                let ctls: Vec<&Ctl> = names
                    .iter()
                    .filter_map(|n| state.ctls.iter().find(|c| c.n == *n))
                    .filter(|c| c.shown)
                    .collect();
                if ctls.is_empty() {
                    continue;
                }
                if !title.is_empty() {
                    ui.add_space(6.0);
                    ui.strong(*title);
                }
                // Labels and multi-line editors get their own lines; small
                // controls share a wrapped row.
                let mut row: Vec<&Ctl> = Vec::new();
                let flush = |ui: &mut egui::Ui,
                             row: &mut Vec<&Ctl>,
                             bufs: &mut EditBufs,
                             change: &mut Option<(String, Value)>| {
                    if row.is_empty() {
                        return;
                    }
                    ui.horizontal_wrapped(|ui| {
                        for c in row.drain(..) {
                            if let Some(v) =
                                pob_controls::show(ui, c, &format!("party_{}", c.n), bufs)
                            {
                                *change = Some((c.n.clone(), v));
                            }
                        }
                    });
                };
                for c in ctls {
                    if c.k == "label" || (c.k == "edit" && c.ml) {
                        flush(ui, &mut row, &mut state.bufs, &mut change);
                        if let Some(v) =
                            pob_controls::show(ui, c, &format!("party_{}", c.n), &mut state.bufs)
                        {
                            change = Some((c.n.clone(), v));
                        }
                    } else {
                        row.push(c);
                    }
                }
                flush(ui, &mut row, &mut state.bufs, &mut change);
            }
        });
    if let Some((name, v)) = change {
        pob.command(pob_controls::set_command(
            &format!("build.partyTab.controls[{}]", lua_str(&name)),
            &v,
        ));
    }
}
