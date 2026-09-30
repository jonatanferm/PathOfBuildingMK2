//! The app's calc backend: Path of Building itself, running headless in
//! `pob_calc`'s interpreter on a worker thread.
//!
//! PoB's build object is the source of truth for calculation. The UI still
//! edits MK2's [`Character`]; [`PobSession::sync`] diffs it against the last
//! state sent to PoB and replays the difference as PoB commands (the same
//! calls PoB's own controls make). Anything without a dedicated command falls
//! back to reloading the whole build from exported XML. Imported builds are
//! handed to PoB verbatim so nothing is lost in translation.

#![cfg(not(target_arch = "wasm32"))]

use std::path::PathBuf;

use pob_calc::worker::{Command, Event, PobHandle, Snapshot};
use pob_data::{Item, Slot};
use pob_engine::{Character, Output};

/// PoB's slot name for an MK2 slot.
#[must_use]
pub fn pob_slot_name(slot: Slot) -> &'static str {
    match slot {
        Slot::Helmet => "Helmet",
        Slot::BodyArmour => "Body Armour",
        Slot::Gloves => "Gloves",
        Slot::Boots => "Boots",
        Slot::Amulet => "Amulet",
        Slot::Ring1 => "Ring 1",
        Slot::Ring2 => "Ring 2",
        Slot::Belt => "Belt",
        Slot::Weapon1 => "Weapon 1",
        Slot::Weapon2 => "Weapon 2",
        Slot::Weapon1Swap => "Weapon 1 Swap",
        Slot::Weapon2Swap => "Weapon 2 Swap",
        Slot::Flask1 => "Flask 1",
        Slot::Flask2 => "Flask 2",
        Slot::Flask3 => "Flask 3",
        Slot::Flask4 => "Flask 4",
        Slot::Flask5 => "Flask 5",
    }
}

/// Quote a string as a Lua literal.
pub fn lua_str(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// Where the worker thread currently stands.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SessionStatus {
    Booting,
    Ready,
    Busy,
    Failed(String),
}

pub struct PobSession {
    handle: PobHandle,
    pub status: SessionStatus,
    /// The character as last sent to PoB (what PoB's build now reflects).
    baseline: Option<Character>,
    /// Sequence number of the newest command sent.
    latest_seq: u64,
    /// Snapshot answering `latest_seq`, once it arrives.
    pub snapshot: Option<Snapshot>,
    /// Boot duration, for the status bar.
    pub boot_millis: Option<u128>,
    /// Last command error from PoB, if any.
    pub last_error: Option<String>,
    /// PoB's current build as an MK2 [`Character`], set when a snapshot
    /// answers the newest command; the UI adopts it via [`Self::take_mirror`].
    mirror: Option<Character>,
    /// Tree version of the mirrored build.
    pub mirror_tree_version: Option<String>,
    /// Sequence of the last verbatim load (open / import): a mirror
    /// answering it reflects the file, not an edit.
    load_seq: Option<u64>,
    /// Whether the pending mirror answers `load_seq`.
    mirror_is_load: bool,
    /// Answers to [`Self::query`], by sequence number.
    query_results: ahash::AHashMap<u64, Result<String, String>>,
}

impl PobSession {
    /// Boot PoB from its `src/` directory on a worker thread.
    #[must_use]
    pub fn spawn(pob_src: PathBuf) -> Self {
        Self {
            handle: PobHandle::spawn(pob_src),
            status: SessionStatus::Booting,
            baseline: None,
            latest_seq: 0,
            snapshot: None,
            boot_millis: None,
            last_error: None,
            mirror: None,
            mirror_tree_version: None,
            load_seq: None,
            mirror_is_load: false,
            query_results: ahash::AHashMap::default(),
        }
    }

    /// Find PoB's sources: `data/pob/src` (packaged) or the dev checkout.
    #[must_use]
    pub fn locate_pob_src(data_root: &std::path::Path) -> Option<PathBuf> {
        let candidates = [
            data_root.join("pob/src"),
            data_root.join("../.PathOfBuilding/src"),
            PathBuf::from(".PathOfBuilding/src"),
        ];
        candidates
            .into_iter()
            .find(|p| p.join("HeadlessWrapper.lua").is_file())
    }

    /// Hand PoB a build verbatim (import / open), and treat `character` (the
    /// MK2 view of the same build) as the new baseline.
    pub fn load_xml(&mut self, xml: String, name: &str, character: &Character) {
        self.latest_seq = self.handle.send(Command::LoadXml {
            xml,
            name: name.to_owned(),
        });
        self.load_seq = Some(self.latest_seq);
        self.baseline = Some(character.clone());
        self.status = SessionStatus::Busy;
    }

    /// Forget PoB's build: the next [`Self::sync`] rebuilds it from scratch
    /// (new build / non-PoB file opened), dropping PoB-only state.
    pub fn reset(&mut self) {
        self.baseline = None;
    }

    /// Bring PoB's build in line with `character`: replay each changed
    /// category as the PoB command its own UI would issue. Categories MK2
    /// models completely but PoB has no single command for (skills, notes)
    /// are reloaded by splicing MK2's section into PoB's current XML, so
    /// nothing PoB-only (config, item sets, …) is lost.
    pub fn sync(&mut self, character: &Character, tree_version: &str) {
        let Some(base) = self.baseline.clone() else {
            // First sync: PoB has only its default build.
            self.full_reload(character);
            return;
        };
        let mut lua = String::new();
        if base.level != character.level {
            lua.push_str(&format!(
                "build.characterLevel = {lvl}\nbuild.characterLevelAutoMode = false\nbuild.controls.characterLevel:SetText({lvl})\nbuild.configTab:BuildModList()\n",
                lvl = character.level.clamp(1, 100)
            ));
        }
        if base.class != character.class
            || base.ascendancy != character.ascendancy
            || base.allocated != character.allocated
            || base.mastery_selections != character.mastery_selections
        {
            lua.push_str(&Self::tree_command(character, tree_version));
        }
        for slot in Slot::all() {
            let old = base.items.get(*slot).map(|i| i.raw.as_str());
            let new = character.items.get(*slot).map(|i| i.raw.as_str());
            if old != new {
                lua.push_str(&Self::equip_command(*slot, character.items.get(*slot)));
            }
        }
        let text = |s: &str| serde_json::Value::String(s.to_owned());
        if base.bandit != character.bandit {
            lua.push_str(&set_config_command(
                "bandit",
                &text(character.bandit.as_pob_name()),
            ));
        }
        if base.pantheon_major != character.pantheon_major {
            lua.push_str(&set_config_command(
                "pantheonMajorGod",
                &text(character.pantheon_major.as_pob_name()),
            ));
        }
        if base.pantheon_minor != character.pantheon_minor {
            lua.push_str(&set_config_command(
                "pantheonMinorGod",
                &text(character.pantheon_minor.as_pob_name()),
            ));
        }

        lua.push_str(&Self::skills_command(&base, character));

        // Notes are reloaded by splicing MK2's section into PoB's own XML.
        let notes_changed = base.notes != character.notes;
        if notes_changed {
            let pob_xml = self
                .snapshot
                .as_ref()
                .map(|s| s.xml.clone())
                .unwrap_or_default();
            if !pob_xml.is_empty() {
                let mk2_xml = pob_engine::export_pob_xml(character);
                let xml = splice_section(&pob_xml, &mk2_xml, "Notes");
                self.latest_seq = self.handle.send(Command::LoadXml {
                    xml,
                    name: "MK2 build".to_owned(),
                });
                if !lua.is_empty() {
                    self.latest_seq = self.handle.send(Command::Lua { code: lua });
                }
                self.baseline = Some(character.clone());
                self.status = SessionStatus::Busy;
                return;
            }
        }
        if lua.is_empty() {
            return;
        }
        self.latest_seq = self.handle.send(Command::Lua { code: lua });
        self.baseline = Some(character.clone());
        self.status = SessionStatus::Busy;
    }

    fn full_reload(&mut self, character: &Character) {
        let xml = pob_engine::export_pob_xml(character);
        self.latest_seq = self.handle.send(Command::LoadXml {
            xml,
            name: "MK2 build".to_owned(),
        });
        self.baseline = Some(character.clone());
        self.status = SessionStatus::Busy;
    }

    /// Socket-group edits as changes to PoB's `skillsTab.socketGroupList`
    /// (groups and gems matched by position), then PoB's own processing.
    fn skills_command(base: &Character, c: &Character) -> String {
        let mut out = String::new();
        let bg = &base.skill_groups;
        let ng = &c.skill_groups;
        let gem_table = |g: &pob_engine::MainSkill| {
            format!(
                "{{ gemId = build.data.gemForSkill[{sid}], skillId = {sid}, nameSpec = \"\", level = {lvl}, quality = {q}, qualityId = {qid}, enabled = {en}, enableGlobal1 = true, enableGlobal2 = true, count = 1 }}",
                sid = lua_str(&g.skill_id),
                lvl = g.level.max(1),
                q = g.quality,
                qid = lua_str(g.quality_id.as_pob_name()),
                en = g.enabled,
            )
        };
        for (i, n) in ng.iter().enumerate() {
            let idx = i + 1;
            let Some(b) = bg.get(i) else {
                // New group.
                let gems: Vec<String> = n.gems.iter().map(gem_table).collect();
                out.push_str(&format!(
                    "table.insert(build.skillsTab.socketGroupList, {{ label = {label}, enabled = {en}, includeInFullDPS = false, mainActiveSkill = {mas}, mainActiveSkillCalcs = {mas}, gemList = {{ {gems} }} }})\n",
                    label = lua_str(&n.label),
                    en = n.enabled,
                    mas = n.main_active_skill_index.max(1),
                    gems = gems.join(", "),
                ));
                continue;
            };
            let sg = format!("build.skillsTab.socketGroupList[{idx}]");
            if b.enabled != n.enabled {
                out.push_str(&format!("{sg}.enabled = {}\n", n.enabled));
            }
            if b.label != n.label {
                out.push_str(&format!("{sg}.label = {}\n", lua_str(&n.label)));
            }
            if b.main_active_skill_index != n.main_active_skill_index {
                out.push_str(&format!(
                    "{sg}.mainActiveSkill = {v}\n{sg}.mainActiveSkillCalcs = {v}\n",
                    v = n.main_active_skill_index.max(1)
                ));
            }
            for (k, g) in n.gems.iter().enumerate() {
                let gk = format!("{sg}.gemList[{}]", k + 1);
                match b.gems.get(k) {
                    None => {
                        out.push_str(&format!("table.insert({sg}.gemList, {})\n", gem_table(g)))
                    }
                    Some(o) if o.skill_id != g.skill_id => {
                        out.push_str(&format!("{gk} = {}\n", gem_table(g)))
                    }
                    Some(o) => {
                        if o.level != g.level {
                            out.push_str(&format!("{gk}.level = {}\n", g.level.max(1)));
                        }
                        if o.quality != g.quality {
                            out.push_str(&format!("{gk}.quality = {}\n", g.quality));
                        }
                        if o.quality_id != g.quality_id {
                            out.push_str(&format!(
                                "{gk}.qualityId = {}\n",
                                lua_str(g.quality_id.as_pob_name())
                            ));
                        }
                        if o.enabled != g.enabled {
                            out.push_str(&format!("{gk}.enabled = {}\n", g.enabled));
                        }
                    }
                }
            }
            for k in (n.gems.len()..b.gems.len()).rev() {
                out.push_str(&format!("table.remove({sg}.gemList, {})\n", k + 1));
            }
        }
        for i in (ng.len()..bg.len()).rev() {
            out.push_str(&format!(
                "table.remove(build.skillsTab.socketGroupList, {})\n",
                i + 1
            ));
        }
        if base.main_socket_group != c.main_socket_group {
            out.push_str(&format!(
                "build.mainSocketGroup = {}\n",
                c.main_socket_group.max(1)
            ));
        }
        if out.is_empty() {
            return out;
        }
        out.push_str(
            "for _, sg in ipairs(build.skillsTab.socketGroupList) do build.skillsTab:ProcessSocketGroup(sg) end\nbuild.skillsTab:AddUndoState()\n",
        );
        out
    }

    fn tree_command(c: &Character, tree_version: &str) -> String {
        let class = c
            .ascendancy
            .as_deref()
            .filter(|a| !a.is_empty())
            .unwrap_or(&c.class.0);
        let mut ids: Vec<u32> = c.allocated.iter().copied().collect();
        ids.sort_unstable();
        let ids = ids
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .join(",");
        let mut masteries: Vec<(u32, u32)> =
            c.mastery_selections.iter().map(|(k, v)| (*k, *v)).collect();
        masteries.sort_unstable();
        let masteries = masteries
            .iter()
            .map(|(k, v)| format!("[{k}]={v}"))
            .collect::<Vec<_>>()
            .join(",");
        format!(
            "do\n  local spec = build.spec\n  spec:ImportFromNodeList({class}, nil, nil, spec.curSecondaryAscendClassId or 0, {{{ids}}}, spec.hashOverrides, {{{masteries}}}, {version})\n  spec:AddUndoState()\nend\n",
            class = lua_str(class),
            version = if tree_version.is_empty() { "nil".to_owned() } else { lua_str(tree_version) },
        )
    }

    fn equip_command(slot: Slot, item: Option<&Item>) -> String {
        let name = lua_str(pob_slot_name(slot));
        match item {
            Some(item) => format!(
                "do\n  local item = new(\"Item\"):Item({raw})\n  if item.base then\n    build.itemsTab:AddItem(item, true)\n    build.itemsTab.slots[{name}]:SetSelItemId(item.id)\n    build.itemsTab:PopulateSlots()\n    build.itemsTab:AddUndoState()\n  end\nend\n",
                raw = lua_str(&item.raw),
            ),
            None => format!(
                "build.itemsTab.slots[{name}]:SetSelItemId(0)\nbuild.itemsTab:PopulateSlots()\nbuild.itemsTab:AddUndoState()\n"
            ),
        }
    }

    /// Drain worker events. Returns true when new outputs arrived.
    pub fn poll(&mut self) -> bool {
        let mut updated = false;
        while let Some(ev) = self.handle.try_recv() {
            match ev {
                Event::Booted { millis } => {
                    self.boot_millis = Some(millis);
                    if self.status == SessionStatus::Booting {
                        self.status = SessionStatus::Ready;
                    }
                }
                Event::BootFailed(e) => self.status = SessionStatus::Failed(e),
                Event::Snapshot(s) => {
                    self.last_error = s.error.clone();
                    if s.seq >= self.latest_seq {
                        self.status = SessionStatus::Ready;
                        // PoB is the source of truth: mirror its build back
                        // into MK2's model once nothing newer is in flight.
                        if let Ok(c) = pob_engine::import_pob_xml(&s.xml) {
                            self.baseline = Some(c.clone());
                            self.mirror = Some(c);
                            self.mirror_is_load = self.load_seq == Some(s.seq);
                            self.mirror_tree_version = pob_engine::pob_tree_version(&s.xml);
                        }
                    }
                    self.snapshot = Some(s);
                    updated = true;
                }
                Event::QueryResult { seq, result } => {
                    self.query_results.insert(seq, result);
                    updated = true;
                }
            }
        }
        updated
    }

    /// PoB's build mirrored into MK2's model, once per snapshot.
    /// The flag is true when the snapshot answered a verbatim file load.
    pub fn take_mirror(&mut self) -> Option<(Character, bool)> {
        self.mirror.take().map(|c| (c, self.mirror_is_load))
    }

    /// Run read-only Lua in PoB; collect the answer with [`Self::take_query`].
    pub fn query(&mut self, code: &str) -> u64 {
        self.handle.send(Command::Query {
            code: code.to_owned(),
        })
    }

    pub fn take_query(&mut self, seq: u64) -> Option<Result<String, String>> {
        self.query_results.remove(&seq)
    }

    /// Run Lua that changes the build (PoB recalculates afterwards).
    pub fn command(&mut self, code: String) {
        self.latest_seq = self.handle.send(Command::Lua { code });
        self.status = SessionStatus::Busy;
    }

    /// PoB's outputs as an MK2 [`Output`], with MK2's legacy key aliases
    /// filled in so existing panels keep working.
    #[must_use]
    pub fn output(&self) -> Option<Output> {
        let s = self.snapshot.as_ref()?;
        let mut out = Output::default();
        for (k, v) in &s.outputs.numbers {
            out.set(k.clone(), *v);
        }
        for (k, v) in &s.outputs.flags {
            out.set(k.clone(), if *v { 1.0 } else { 0.0 });
        }
        let n = |k: &str| s.outputs.numbers.get(k).copied();
        for (alias, key) in [
            ("Strength", "Str"),
            ("Dexterity", "Dex"),
            ("Intelligence", "Int"),
            ("MainSkillDPS", "TotalDPS"),
            ("MainSkillAverageHit", "AverageHit"),
            ("MainSkillAverageHitWithCrit", "AverageDamage"),
            ("MainSkillSpeed", "Speed"),
            ("MainSkillCritChance", "CritChance"),
            ("MainSkillHitChance", "HitChance"),
            ("MainSkillManaCost", "ManaCost"),
            ("MinionDPS", "MinionTotalDPS"),
            ("MinionAttacksPerSecond", "MinionSpeed"),
            ("MinionAverageDamage", "MinionAverageDamage"),
            ("MinionCritFactor", "MinionCritEffect"),
        ] {
            if let Some(v) = n(key) {
                out.set(alias, v);
            }
        }
        let dot_only = n("TotalDPS").unwrap_or(0.0) == 0.0 && n("TotalDot").unwrap_or(0.0) > 0.0;
        out.set("MainSkillIsDotOnly", if dot_only { 1.0 } else { 0.0 });
        Some(out)
    }
}

/// Lua that lists PoB's config options (sections, types, labels, current
/// values, visibility as PoB's own Config tab decides it) as JSON.
pub const CONFIG_QUERY: &str = r#"
local varList = require("Modules.ConfigOptions")
local tab = build.configTab
local set = tab.configSets[tab.activeConfigSetId]
local function esc(s)
    s = tostring(s):gsub("%^x%x%x%x%x%x%x", ""):gsub("%^%d", "")
    return '"' .. s:gsub('[%c"\\]', function(c)
        if c == '"' then return '\\"' elseif c == "\\" then return "\\\\" elseif c == "\n" then return "\\n" end
        return ""
    end) .. '"'
end
local function val(v)
    local t = type(v)
    if t == "boolean" then return tostring(v)
    elseif t == "number" then return string.format("%.10g", v)
    elseif t == "string" then return esc(v) end
    return "null"
end
local out, section = {}, ""
for _, vd in ipairs(varList) do
    if vd.section then
        section = vd.section
    elseif vd.var then
        local ctrl = tab.varControls[vd.var]
        local shown = true
        if ctrl then
            local ok, s = pcall(function() return ctrl:IsShown() end)
            shown = ok and s and true or false
        end
        local tip = nil
        if type(vd.tooltip) == "string" then tip = vd.tooltip end
        if ctrl and ctrl.tooltipText then
            local ok, t = pcall(function() return type(ctrl.tooltipText) == "function" and ctrl.tooltipText() or ctrl.tooltipText end)
            if ok and type(t) == "string" then tip = t end
        end
        local opts = {}
        if vd.list then
            for _, o in ipairs(vd.list) do opts[#opts + 1] = "[" .. val(o.val) .. "," .. esc(o.label or o.val) .. "]" end
        end
        out[#out + 1] = "{" .. table.concat({
            '"s":' .. esc(section), '"v":' .. esc(vd.var), '"t":' .. esc(vd.type or ""),
            '"l":' .. esc(vd.label or vd.var), '"sh":' .. tostring(shown),
            '"val":' .. val(set.input[vd.var]), '"ph":' .. val(set.placeholder[vd.var]),
            '"opts":[' .. table.concat(opts, ",") .. "]",
            '"tip":' .. (tip and esc(tip) or "null"),
        }, ",") .. "}"
    end
end
return "[" .. table.concat(out, ",") .. "]"
"#;

/// One of PoB's config options.
#[derive(Debug, Clone, serde::Deserialize)]
pub struct ConfigOption {
    #[serde(rename = "s")]
    pub section: String,
    #[serde(rename = "v")]
    pub var: String,
    #[serde(rename = "t")]
    pub ty: String,
    #[serde(rename = "l")]
    pub label: String,
    #[serde(rename = "sh")]
    pub shown: bool,
    #[serde(rename = "val")]
    pub value: Option<serde_json::Value>,
    #[serde(rename = "ph")]
    pub placeholder: Option<serde_json::Value>,
    pub opts: Vec<(serde_json::Value, String)>,
    pub tip: Option<String>,
}

/// Lua that sets a config input the way PoB's Config tab controls do.
#[must_use]
pub fn set_config_command(var: &str, value: &serde_json::Value) -> String {
    let lua_value = match value {
        serde_json::Value::Bool(b) => b.to_string(),
        serde_json::Value::Number(n) => n.to_string(),
        serde_json::Value::String(s) => lua_str(s),
        _ => "nil".to_owned(),
    };
    format!(
        "do\n  local tab = build.configTab\n  tab.configSets[tab.activeConfigSetId].input[{var}] = {lua_value}\n  tab:UpdateControls()\n  tab:AddUndoState()\n  tab:BuildModList()\nend\n",
        var = lua_str(var)
    )
}

/// Lua listing the Calcs tab exactly as PoB lays it out: sections (in PoB's
/// column groups), sub-sections and rows enabled for the current skill, with
/// each cell formatted by PoB's `formatCalcStr`. Cells are addressed by
/// `[section, subsection, row, column]` for [`breakdown_query`].
pub const CALCS_QUERY: &str = r#"
local tab = build.calcsTab
local env = tab.calcsEnv or tab.mainEnv
local actor = tab.input.showMinion and env.minion or env.player
local out = {}
for si, section in ipairs(tab.sectionList) do
    pcall(function() section:UpdateSize() end)
    if section.enabled then
        local subs = {}
        for subi, subSec in ipairs(section.subSection) do
            local rows = {}
            for ri, row in ipairs(subSec.data) do
                if tab:CheckFlag(row) then
                    local cells = {}
                    local hidden = false
                    for ci, col in ipairs(row) do
                        if col.format and tab:CheckFlag(col) then
                            local ok, text = pcall(formatCalcStr, col.format, actor, col)
                            cells[#cells + 1] = { t = ok and text or "?", r = { si, subi, ri, ci }, b = #col > 0 }
                        elseif col.controlName and section.controls[col.controlName] then
                            local c = __mk2_ctl(col.controlName, section.controls[col.controlName])
                            hidden = hidden or not c.shown
                            cells[#cells + 1] = { r = { si, subi, ri, ci }, ctl = c }
                        end
                    end
                    if not hidden then
                        rows[#rows + 1] = { l = row.label or "", c = row.color, cells = cells }
                    end
                end
            end
            local extra
            if subSec.data.extra then
                local ok, text = pcall(formatCalcStr, subSec.data.extra, actor)
                extra = ok and text or nil
            end
            subs[#subs + 1] = { l = subSec.label or "", x = extra, rows = rows }
        end
        out[#out + 1] = { id = section.id, g = section.group, col = section.colour, subs = subs }
    end
end
return __mk2_json(out)
"#;

/// Lua for PoB's breakdown of one Calcs cell.
#[must_use]
pub fn breakdown_query(r: [u32; 4]) -> String {
    format!(
        r#"
local tab = build.calcsTab
local col = tab.sectionList[{0}].subSection[{1}].data[{2}][{3}]
local bd = tab.controls.breakdown
bd.sourceData = nil
bd:SetBreakdownData(col, true)
local out = {{}}
for _, s in ipairs(bd.sectionList or {{}}) do
    if s.type == "TEXT" then
        out[#out + 1] = {{ type = "TEXT", lines = s.lines }}
    elseif s.type == "TABLE" then
        local cols, rows = {{}}, {{}}
        for _, c in ipairs(s.colList or {{}}) do cols[#cols + 1] = {{ key = c.key, label = c.label }} end
        for _, row in ipairs(s.rowList or {{}}) do
            local r = {{}}
            for _, c in ipairs(s.colList or {{}}) do r[#r + 1] = row[c.key] and tostring(row[c.key]) or "" end
            rows[#rows + 1] = r
        end
        out[#out + 1] = {{ type = "TABLE", label = s.label, cols = cols, rows = rows, footer = s.footer }}
    end
end
return __mk2_json(out)
"#,
        r[0], r[1], r[2], r[3]
    )
}

#[derive(Debug, Clone, serde::Deserialize)]
pub struct CalcCell {
    #[serde(default)]
    pub t: String,
    /// A PoB control in place of text (the skill-select section).
    #[serde(default)]
    pub ctl: Option<crate::pob_controls::Ctl>,
    pub r: [u32; 4],
    #[serde(default)]
    pub b: bool,
}

#[derive(Debug, Clone, serde::Deserialize)]
pub struct CalcRow {
    pub l: String,
    #[serde(default)]
    pub c: Option<String>,
    #[serde(default)]
    pub cells: Vec<CalcCell>,
}

#[derive(Debug, Clone, serde::Deserialize)]
pub struct CalcSub {
    pub l: String,
    #[serde(default)]
    pub x: Option<String>,
    #[serde(default)]
    pub rows: Vec<CalcRow>,
}

#[derive(Debug, Clone, serde::Deserialize)]
pub struct CalcSection {
    #[serde(default)]
    pub id: Option<String>,
    #[serde(default)]
    pub g: Option<u32>,
    #[serde(default)]
    pub col: Option<String>,
    #[serde(default)]
    pub subs: Vec<CalcSub>,
}

#[derive(Debug, Clone, serde::Deserialize)]
pub struct BreakdownSection {
    #[serde(rename = "type")]
    pub ty: String,
    #[serde(default)]
    pub lines: Vec<String>,
    #[serde(default)]
    pub label: Option<String>,
    #[serde(default)]
    pub cols: Vec<BreakdownCol>,
    #[serde(default)]
    pub rows: Vec<Vec<String>>,
    #[serde(default)]
    pub footer: Option<String>,
}

#[derive(Debug, Clone, serde::Deserialize)]
pub struct BreakdownCol {
    #[serde(default)]
    pub key: Option<String>,
    #[serde(default)]
    pub label: Option<String>,
}

/// Byte range of the top-level `<name …>…</name>` / `<name …/>` element.
fn find_section(xml: &str, name: &str) -> Option<(usize, usize)> {
    let open = format!("<{name}");
    let mut from = 0;
    let start = loop {
        let i = from + xml[from..].find(&open)?;
        let after = xml.as_bytes().get(i + open.len()).copied();
        if matches!(after, Some(b' ' | b'>' | b'/' | b'\n' | b'\t' | b'\r')) {
            break i;
        }
        from = i + open.len();
    };
    let tag_end = start + xml[start..].find('>')?;
    if xml.as_bytes()[tag_end - 1] == b'/' {
        return Some((start, tag_end + 1));
    }
    let close = format!("</{name}>");
    let end = tag_end + xml[tag_end..].find(&close)? + close.len();
    Some((start, end))
}

/// Replace `name`'s element in `target` with the one from `source` (or
/// insert it before `</PathOfBuilding>` when `target` lacks it).
#[must_use]
pub fn splice_section(target: &str, source: &str, name: &str) -> String {
    let Some((ss, se)) = find_section(source, name) else {
        return target.to_owned();
    };
    let replacement = &source[ss..se];
    match find_section(target, name) {
        Some((ts, te)) => format!("{}{}{}", &target[..ts], replacement, &target[te..]),
        None => match target.rfind("</PathOfBuilding>") {
            Some(i) => format!("{}{}\n{}", &target[..i], replacement, &target[i..]),
            None => target.to_owned(),
        },
    }
}

/// Set an attribute on the `<Build …>` start tag.
#[must_use]
pub fn set_build_attr(xml: &str, attr: &str, value: &str) -> String {
    let Some(start) = xml.find("<Build") else {
        return xml.to_owned();
    };
    let Some(rel_end) = xml[start..].find('>') else {
        return xml.to_owned();
    };
    let tag = &xml[start..start + rel_end];
    let needle = format!("{attr}=\"");
    let new_tag = if let Some(a) = tag.find(&needle) {
        let vstart = a + needle.len();
        let vend = vstart + tag[vstart..].find('"').unwrap_or(0);
        format!("{}{}{}", &tag[..vstart], value, &tag[vend..])
    } else {
        let (body, slash) = tag.strip_suffix('/').map_or((tag, ""), |b| (b, "/"));
        format!("{body} {attr}=\"{value}\"{slash}")
    };
    format!("{}{}{}", &xml[..start], new_tag, &xml[start + rel_end..])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn splices_sections_and_build_attrs() {
        let target = "<PathOfBuilding><Build level=\"90\" mainSocketGroup=\"1\"/><Skills a=\"1\"><Skill/></Skills><Config><Input name=\"x\"/></Config></PathOfBuilding>";
        let source =
            "<PathOfBuilding><Skills><Skill id=\"2\"/></Skills><Notes>hi</Notes></PathOfBuilding>";
        let out = splice_section(target, source, "Skills");
        assert!(out.contains("<Skills><Skill id=\"2\"/></Skills>"));
        assert!(out.contains("<Config><Input name=\"x\"/></Config>"));
        let out = splice_section(&out, source, "Notes");
        assert!(out.contains("<Notes>hi</Notes>\n</PathOfBuilding>"));
        let out = set_build_attr(&out, "mainSocketGroup", "3");
        assert!(out.contains("mainSocketGroup=\"3\""));
        assert!(!out.contains("mainSocketGroup=\"1\""));
    }
}
