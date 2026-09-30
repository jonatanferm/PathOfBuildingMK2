//! The Items tab's line to Path of Building: item tooltips with PoB's
//! equip comparisons, and PoB's unique / rare item databases.

#![cfg(not(target_arch = "wasm32"))]

use pob_data::{Item, Slot};

use crate::items_tab::{DbEntry, PobItems};
use crate::pob_session::{lua_str, pob_slot_name, PobSession};
use crate::pob_tooltip::{self, Fetcher, Line};

/// Loaded databases and PoB's slot list, kept across frames.
#[derive(Default)]
pub struct PobItemDbState {
    lists: [Option<Vec<DbEntry>>; 2],
    pending: [Option<u64>; 2],
    slots: Option<SlotList>,
    slots_pending: Option<u64>,
    slots_for: Option<u64>,
    bufs: crate::pob_controls::EditBufs,
}

#[derive(Debug, Clone, Default, serde::Deserialize)]
struct SlotList {
    #[serde(default)]
    swap: bool,
    #[serde(default)]
    slots: Vec<SlotRow>,
}

#[derive(Debug, Clone, serde::Deserialize)]
struct SlotRow {
    #[serde(flatten)]
    ctl: crate::pob_controls::Ctl,
    #[serde(default)]
    item: String,
    #[serde(default)]
    abyssal: bool,
    #[serde(default)]
    jewel: bool,
    #[serde(default)]
    flask: bool,
    #[serde(default)]
    active: bool,
}

/// MK2's slot for a PoB slot name, when it has one.
fn mk2_slot(name: &str) -> Option<Slot> {
    Slot::all()
        .iter()
        .copied()
        .find(|s| pob_slot_name(*s) == name)
}

pub struct Bridge<'a> {
    pub pob: Option<&'a mut PobSession>,
    pub tips: &'a mut Fetcher,
    pub db: &'a mut PobItemDbState,
    pub ctx: egui::Context,
}

impl Bridge<'_> {
    fn tip(&mut self, key: &str, lua: impl FnOnce() -> String) -> Option<Vec<Line>> {
        let pob = self.pob.as_deref_mut()?;
        let lines = self.tips.get(pob, key, lua).map(<[Line]>::to_vec);
        if self.tips.busy() {
            self.ctx
                .request_repaint_after(std::time::Duration::from_millis(30));
        }
        lines.filter(|l| !l.is_empty())
    }
}

fn db_expr(rare: bool) -> &'static str {
    if rare {
        "__mainObject__.main.rareDB"
    } else {
        "__mainObject__.main.uniqueDB"
    }
}

impl PobItems for Bridge<'_> {
    fn tooltip(&mut self, item: &Item, slot: Option<Slot>) -> Option<Vec<Line>> {
        let slot_name = slot.map(pob_slot_name);
        let key = {
            use std::hash::{Hash, Hasher};
            let mut h = std::collections::hash_map::DefaultHasher::new();
            item.raw.hash(&mut h);
            format!("item:{slot_name:?}:{}", h.finish())
        };
        self.tip(&key, || pob_tooltip::item_query(&item.raw, slot_name))
    }

    fn db(&mut self, rare: bool) -> Option<&[DbEntry]> {
        let i = usize::from(rare);
        let pob = self.pob.as_deref_mut()?;
        if self.db.lists[i].is_none() {
            match self.db.pending[i] {
                None => {
                    self.db.pending[i] = Some(pob.query(&format!(
                        r#"
__mk2_load_item_dbs()
local out = {{}}
for name, item in pairs({db}.list) do
    local mods = {{}}
    for _, list in ipairs({{ item.implicitModLines or {{}}, item.explicitModLines or {{}} }}) do
        for _, l in ipairs(list) do mods[#mods + 1] = l.line end
    end
    out[#out + 1] = {{ n = name, t = item.type or "", b = item.baseName or "", l = item.requirements and item.requirements.level or 0, m = table.concat(mods, "\n") }}
end
table.sort(out, function(a, b) return a.n < b.n end)
return __mk2_json(out)
"#,
                        db = db_expr(rare)
                    )));
                }
                Some(seq) => {
                    if let Some(res) = pob.take_query(seq) {
                        self.db.pending[i] = None;
                        self.db.lists[i] = Some(
                            res.ok()
                                .and_then(|j| serde_json::from_str(&j).ok())
                                .unwrap_or_default(),
                        );
                    }
                }
            }
            self.ctx
                .request_repaint_after(std::time::Duration::from_millis(50));
        }
        self.db.lists[i].as_deref()
    }

    fn db_tooltip(&mut self, rare: bool, name: &str) -> Option<Vec<Line>> {
        let key = format!("db:{rare}:{name}");
        let name = lua_str(name);
        let db = db_expr(rare);
        self.tip(&key, || {
            format!(
                r#"
local item = {db}.list[{name}]
if not item then return "[]" end
local tt = new("Tooltip"):Tooltip()
build.itemsTab:AddItemTooltip(tt, item, nil, true)
return __mk2_tooltip(tt)
"#
            )
        })
    }

    fn db_equip(&mut self, rare: bool, name: &str, slot: Option<Slot>) {
        let Some(pob) = self.pob.as_deref_mut() else {
            return;
        };
        let slot = slot.map_or_else(|| "nil".to_owned(), |s| lua_str(pob_slot_name(s)));
        pob.command(format!(
            r#"
local src = {db}.list[{name}]
if src then
    local itemsTab = build.itemsTab
    local item = new("Item"):Item(src:BuildRaw())
    item:NormaliseQuality()
    local slotName = {slot}
    if slotName and itemsTab.slots[slotName] and itemsTab:IsItemValidForSlot(item, slotName) then
        itemsTab:AddItem(item, true)
        itemsTab.slots[slotName]:SetSelItemId(item.id)
    else
        itemsTab:AddItem(item, false)
    end
    itemsTab:PopulateSlots()
    itemsTab:AddUndoState()
end
"#,
            db = db_expr(rare),
            name = lua_str(name),
        ));
    }

    fn available(&self) -> bool {
        self.pob.as_ref().is_some_and(|p| p.snapshot.is_some())
    }

    fn draw_slots(&mut self, ui: &mut egui::Ui, selected: &mut Option<Slot>) -> bool {
        if !self.available() {
            return false;
        }
        let Some(pob) = self.pob.as_deref_mut() else {
            return false;
        };
        let snap = pob.snapshot.as_ref().map(|s| s.seq);
        if self.db.slots_pending.is_none() && self.db.slots_for != snap {
            self.db.slots_pending = Some(pob.query("return __mk2_item_slots()"));
            self.db.slots_for = snap;
        }
        if let Some(seq) = self.db.slots_pending {
            if let Some(res) = pob.take_query(seq) {
                self.db.slots_pending = None;
                if let Some(list) = res.ok().and_then(|j| serde_json::from_str(&j).ok()) {
                    self.db.slots = Some(list);
                    self.db.bufs.clear();
                }
            } else {
                self.ctx
                    .request_repaint_after(std::time::Duration::from_millis(30));
            }
        }
        let Some(list) = self.db.slots.clone() else {
            ui.spinner();
            return true;
        };
        let mut commands: Vec<String> = Vec::new();
        ui.horizontal(|ui| {
            ui.label("Weapon set:");
            for (swap, label) in [(false, "I"), (true, "II")] {
                if ui.selectable_label(list.swap == swap, label).clicked() && list.swap != swap {
                    commands.push(format!("__mk2_item_slot_op('swap', nil, {swap})\n"));
                }
            }
        });
        let mut last_jewel_header = false;
        // The surrounding horizontal layout under-reports free height; size
        // the list from the window instead, leaving room for the buttons.
        let max_h = (ui.ctx().screen_rect().bottom() - ui.cursor().top() - 110.0).max(160.0);
        egui::ScrollArea::vertical()
            .id_salt("pob_item_slots")
            .max_height(max_h)
            .min_scrolled_height(max_h)
            .auto_shrink([false, true])
            .show(ui, |ui| {
                for row in &list.slots {
                    if row.jewel && !last_jewel_header {
                        ui.add_space(4.0);
                        ui.weak("Tree jewel sockets");
                        last_jewel_header = true;
                    }
                    let name = row.ctl.n.clone();
                    ui.horizontal(|ui| {
                        if row.abyssal {
                            ui.add_space(14.0);
                        }
                        if row.flask {
                            let mut on = row.active;
                            if ui
                                .checkbox(&mut on, "")
                                .on_hover_text("Activate this flask.")
                                .changed()
                            {
                                commands.push(format!(
                                    "__mk2_item_slot_op('activate', {}, {on})\n",
                                    lua_str(&name)
                                ));
                            }
                        }
                        let label = row.ctl.label.clone().unwrap_or_else(|| name.clone());
                        let mapped = mk2_slot(&name);
                        let is_sel = mapped.is_some() && *selected == mapped;
                        let resp = ui.add_sized(
                            [72.0, 18.0],
                            egui::SelectableLabel::new(is_sel, egui::RichText::new(label).small()),
                        );
                        if resp.clicked() {
                            if let Some(m) = mapped {
                                *selected = Some(m);
                            }
                        }
                        if !row.item.is_empty() && resp.hovered() {
                            let key = format!("slot:{name}:{}", row.item);
                            let lines = self
                                .tips
                                .get(pob, &key, || {
                                    format!("return __mk2_slot_tooltip({})", lua_str(&name))
                                })
                                .map(<[Line]>::to_vec);
                            if self.tips.busy() {
                                self.ctx
                                    .request_repaint_after(std::time::Duration::from_millis(30));
                            }
                            resp.on_hover_ui(|ui| {
                                ui.set_max_width(520.0);
                                match &lines {
                                    Some(l) => pob_tooltip::render(ui, l),
                                    None => {
                                        ui.spinner();
                                    }
                                }
                            });
                        }
                        if let Some(v) = crate::pob_controls::show(
                            ui,
                            &row.ctl,
                            &format!("slot_{name}"),
                            &mut self.db.bufs,
                        ) {
                            commands.push(format!(
                                "__mk2_item_slot_op('sel', {}, {})\n",
                                lua_str(&name),
                                match v {
                                    serde_json::Value::Number(n) => n.to_string(),
                                    _ => "nil".to_owned(),
                                }
                            ));
                        }
                    });
                }
            });
        for c in commands {
            pob.command(c);
        }
        true
    }
}
