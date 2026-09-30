//! Path of Building's own tooltips (passive nodes, items, gems), including
//! its "Allocating this node will give you: …" stat comparisons. PoB builds
//! them with its `Tooltip` class on the worker; they arrive here as sized,
//! colour-coded lines.

/// One tooltip line; `t: None` is a separator.
#[derive(Debug, Clone, serde::Deserialize)]
pub struct Line {
    #[serde(default = "default_size")]
    pub s: f32,
    #[serde(default)]
    pub t: Option<String>,
}

fn default_size() -> f32 {
    16.0
}

/// Parse `__mk2_tooltip` output (empty on error).
#[must_use]
pub fn parse(json: &str) -> Vec<Line> {
    serde_json::from_str(json).unwrap_or_default()
}

/// Draw the lines (PoB font sizes scaled to egui's).
pub fn render(ui: &mut egui::Ui, lines: &[Line]) {
    ui.spacing_mut().item_spacing.y = 1.0;
    for l in lines {
        match &l.t {
            Some(t) if t.is_empty() => {
                ui.add_space(l.s * 0.3);
            }
            Some(t) => {
                ui.label(crate::pob_sidebar::job(t, (l.s * 0.85).clamp(10.0, 22.0)));
            }
            None => {
                ui.separator();
            }
        }
    }
}

/// Asks PoB for one tooltip at a time and caches the answer per key and
/// snapshot, so hovering is instant once computed and refreshes after
/// every build change.
#[cfg(not(target_arch = "wasm32"))]
#[derive(Default)]
pub struct Fetcher {
    pending: Option<(String, u64)>,
    ready: Option<(String, Option<u64>, Vec<Line>)>,
}

#[cfg(not(target_arch = "wasm32"))]
impl Fetcher {
    /// The tooltip for `key` (built by `lua` if not cached). Returns the
    /// last answer for `key` while a refresh is in flight.
    pub fn get(
        &mut self,
        pob: &mut crate::pob_session::PobSession,
        key: &str,
        lua: impl FnOnce() -> String,
    ) -> Option<&[Line]> {
        if let Some((k, qseq)) = &self.pending {
            if let Some(res) = pob.take_query(*qseq) {
                let lines = res.map(|j| parse(&j)).unwrap_or_default();
                let snap = pob.snapshot.as_ref().map(|s| s.seq);
                self.ready = Some((k.clone(), snap, lines));
                self.pending = None;
            }
        }
        let snap = pob.snapshot.as_ref().map(|s| s.seq);
        let fresh = self
            .ready
            .as_ref()
            .is_some_and(|(k, s, _)| k == key && *s == snap);
        let in_flight = self.pending.as_ref().is_some_and(|(k, _)| k == key);
        if !fresh && !in_flight && self.pending.is_none() && snap.is_some() {
            self.pending = Some((key.to_owned(), pob.query(&lua())));
        }
        self.ready
            .as_ref()
            .filter(|(k, _, _)| k == key)
            .map(|(_, _, l)| l.as_slice())
    }

    /// Whether a query is outstanding (keep repainting).
    #[must_use]
    pub fn busy(&self) -> bool {
        self.pending.is_some()
    }
}

/// Lua for a passive node's tooltip.
#[cfg(not(target_arch = "wasm32"))]
#[must_use]
pub fn node_query(node_id: u32) -> String {
    format!(
        r#"
local node = build.spec.nodes[{node_id}]
if not node then return "[]" end
local tt = new("Tooltip"):Tooltip()
build.treeTab.viewer:AddNodeTooltip(tt, node, build)
return __mk2_tooltip(tt)
"#
    )
}

/// Lua for an item's tooltip: the item PoB has in `slot` when given (the
/// equipped one), otherwise `raw` parsed as a new item — either way with
/// PoB's "Equipping this item … will give you" comparisons.
#[cfg(not(target_arch = "wasm32"))]
#[must_use]
pub fn item_query(raw: &str, slot: Option<&str>) -> String {
    let slot = slot.map_or_else(|| "nil".to_owned(), crate::pob_session::lua_str);
    format!(
        r#"
local itemsTab = build.itemsTab
local slotName = {slot}
local slot = slotName and itemsTab.slots[slotName]
local item = slot and itemsTab.items[slot.selItemId]
if not item then
    item = new("Item"):Item({raw})
    item:NormaliseQuality()
    item:BuildModList()
end
local tt = new("Tooltip"):Tooltip()
itemsTab:AddItemTooltip(tt, item, slot)
return __mk2_tooltip(tt)
"#,
        raw = crate::pob_session::lua_str(raw),
    )
}
