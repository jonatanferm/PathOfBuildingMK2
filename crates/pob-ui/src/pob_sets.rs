//! Path of Building's tree specs, item sets, skill sets and config sets:
//! a selector plus New / Copy / Rename / Delete, each performed by PoB's own
//! set-management code.

#![cfg(not(target_arch = "wasm32"))]

use crate::pob_session::{lua_str, PobSession};
use crate::pob_sidebar::job;

#[derive(Debug, Clone, Default, serde::Deserialize)]
struct SetList {
    #[serde(default)]
    items: Vec<String>,
    #[serde(default)]
    sel: usize,
}

#[derive(Debug, Clone, Default, serde::Deserialize)]
struct Sets {
    #[serde(default)]
    spec: SetList,
    #[serde(default)]
    item: SetList,
    #[serde(default)]
    skill: SetList,
    #[serde(default)]
    config: SetList,
}

/// Which of PoB's set lists.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Spec,
    Item,
    Skill,
    Config,
}

impl Kind {
    fn key(self) -> &'static str {
        match self {
            Self::Spec => "spec",
            Self::Item => "item",
            Self::Skill => "skill",
            Self::Config => "config",
        }
    }
    fn noun(self) -> &'static str {
        match self {
            Self::Spec => "passive tree",
            Self::Item => "item set",
            Self::Skill => "skill set",
            Self::Config => "config set",
        }
    }
}

/// A pending name prompt / delete confirmation.
struct Prompt {
    kind: Kind,
    op: &'static str,
    index: Option<usize>,
    name: String,
}

#[derive(Default)]
pub struct PobSetsState {
    prompt: Option<Prompt>,
}

fn lists(pob: &PobSession) -> Option<Sets> {
    let s = pob.snapshot.as_ref()?;
    serde_json::from_str(&s.sets).ok()
}

fn op_command(kind: Kind, op: &str, index: Option<usize>, name: Option<&str>) -> String {
    format!(
        "__mk2_set_op({}, {}, {}, {})\n",
        lua_str(kind.key()),
        lua_str(op),
        index.map_or_else(|| "nil".to_owned(), |i| i.to_string()),
        name.map_or_else(|| "nil".to_owned(), lua_str),
    )
}

/// One set row: `label [selector] New Copy Rename Delete`. Returns true
/// when a command was sent to PoB.
pub fn row(
    ui: &mut egui::Ui,
    state: &mut PobSetsState,
    pob: &mut PobSession,
    kind: Kind,
    label: &str,
) -> bool {
    let Some(sets) = lists(pob) else {
        return false;
    };
    let list = match kind {
        Kind::Spec => sets.spec,
        Kind::Item => sets.item,
        Kind::Skill => sets.skill,
        Kind::Config => sets.config,
    };
    let mut command = None;
    ui.horizontal_wrapped(|ui| {
        ui.label(label);
        let current = list
            .items
            .get(list.sel.saturating_sub(1))
            .cloned()
            .unwrap_or_default();
        egui::ComboBox::from_id_salt(("pob_sets", kind.key()))
            .selected_text(job(&current, 13.0))
            .width(200.0)
            .show_ui(ui, |ui| {
                for (i, title) in list.items.iter().enumerate() {
                    if ui
                        .selectable_label(i + 1 == list.sel, job(title, 13.0))
                        .clicked()
                        && i + 1 != list.sel
                    {
                        command = Some(op_command(kind, "select", Some(i + 1), None));
                    }
                }
            });
        let sel = Some(list.sel.max(1));
        if ui
            .small_button("New")
            .on_hover_text(format!("Add a new {}", kind.noun()))
            .clicked()
        {
            state.prompt = Some(Prompt {
                kind,
                op: "new",
                index: None,
                name: String::new(),
            });
        }
        if ui.small_button("Copy").clicked() {
            state.prompt = Some(Prompt {
                kind,
                op: "copy",
                index: sel,
                name: format!("Copy of {}", strip_codes(&current)),
            });
        }
        if ui.small_button("Rename").clicked() {
            state.prompt = Some(Prompt {
                kind,
                op: "rename",
                index: sel,
                name: strip_codes(&current),
            });
        }
        if ui
            .add_enabled(list.items.len() > 1, egui::Button::new("Delete").small())
            .clicked()
        {
            state.prompt = Some(Prompt {
                kind,
                op: "delete",
                index: sel,
                name: strip_codes(&current),
            });
        }
    });

    // Name prompt / delete confirmation for this row.
    let mut close = false;
    if let Some(p) = state.prompt.as_mut().filter(|p| p.kind == kind) {
        let title = match p.op {
            "new" => format!("New {}", kind.noun()),
            "copy" => format!("Copy {}", kind.noun()),
            "rename" => format!("Rename {}", kind.noun()),
            _ => format!("Delete {}", kind.noun()),
        };
        egui::Window::new(title)
            .id(egui::Id::new(("pob_sets_prompt", kind.key())))
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
            .show(ui.ctx(), |ui| {
                if p.op == "delete" {
                    ui.label(format!("Are you sure you want to delete '{}'?", p.name));
                } else {
                    ui.label(format!("Enter name for this {}:", kind.noun()));
                    let r = ui.add(egui::TextEdit::singleline(&mut p.name).desired_width(300.0));
                    r.request_focus();
                }
                ui.horizontal(|ui| {
                    let ok_label = if p.op == "delete" { "Delete" } else { "Save" };
                    let can = p.op == "delete" || !p.name.trim().is_empty();
                    let enter = ui.input(|i| i.key_pressed(egui::Key::Enter));
                    if ui.add_enabled(can, egui::Button::new(ok_label)).clicked() || (can && enter)
                    {
                        let name = (p.op != "delete").then_some(p.name.as_str());
                        command = Some(op_command(kind, p.op, p.index, name));
                        close = true;
                    }
                    if ui.button("Cancel").clicked()
                        || ui.input(|i| i.key_pressed(egui::Key::Escape))
                    {
                        close = true;
                    }
                });
            });
    }
    if close {
        state.prompt = None;
    }
    match command {
        Some(c) => {
            pob.command(c);
            true
        }
        None => false,
    }
}

fn strip_codes(s: &str) -> String {
    crate::pob_sidebar::runs(s, egui::Color32::WHITE)
        .into_iter()
        .map(|(t, _)| t)
        .collect()
}
