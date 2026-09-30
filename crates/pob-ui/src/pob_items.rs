//! The Items tab's line to Path of Building: item tooltips with PoB's
//! equip comparisons, and PoB's unique / rare item databases.

#![cfg(not(target_arch = "wasm32"))]

use pob_data::{Item, Slot};

use crate::items_tab::{DbEntry, PobItems};
use crate::pob_session::{lua_str, pob_slot_name, PobSession};
use crate::pob_tooltip::{self, Fetcher, Line};

/// Loaded databases, kept across frames.
#[derive(Default)]
pub struct PobItemDbState {
    lists: [Option<Vec<DbEntry>>; 2],
    pending: [Option<u64>; 2],
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
}
