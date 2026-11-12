//! U48 acceptance (ui doc §12 U48 row: CompareTab C, CompareTab.lua:2516-3161,
//! ComparePowerReportListControl.lua, CompareCalcsHelpers.lua; "Compare power report identical
//! through the bridge"): the full app (Launch, the build list, the build shell, the Compare tab)
//! on both sides of the UI oracle, in u46's form. Owner: U48.
//!
//! `u48_scenarios`: every `tests/u48/*.pobscript` (each sets its own `screen`). Each script opens
//! `allflame_exp_001` from `tests/u48/user`, presses the Compare button, imports
//! `Sub/allflame_exp_030` through the folder popup (U46) and drives the Summary view's power
//! report: the metric dropdown, the category check boxes, the report list (rows, hover
//! tooltips, the sortable column headers) and the Calcs sub-tab's helper tooltips. Per run: zero
//! T3 divergences (draw ops per `(layer, sub)` bucket and side effects, frame by frame) and the
//! same window titles.
//!
//! What runs on each side:
//! * `ComparePowerBuilder`, `RunComparePowerReport` (the coroutine created, resumed and dropped
//!   when dead), `ComparePowerReportListControl` (SetReport, SetProgress, ReList, ReSort, Draw,
//!   GetRowValue, AddValueTooltip) and `CompareCalcsHelpers` (DrawCalcsTooltip with
//!   FormatModRow/ModRowKey/TabulateMods/GetModStoreAndCfg/GetBreakdownLines/ResolveSourceName,
//!   DrawSkillBreakdownPanel): PoB's (Lua) and U48's (Rust);
//! * `CompareTab:Draw` (U47) and `DrawSummary` (U49) are a stand-in on both sides
//!   ([`LUA_STUBS`] and [`draw_compare`]): the background, the sub-tab anchor, the call of
//!   `RunComparePowerReport` and DrawSummary's list-sync lines (CompareTab.lua:3274-3299) with
//!   fixed control positions, `ProcessControlsInput`, `DrawControls`, and status lines that print
//!   the report state and every result entry (category, name, combined impact, points, per
//!   point, percent; sorted as strings so the lines do not depend on the builder's `pairs`
//!   order, power.rs ORDER note). In the Calcs sub-tab the stand-in draws the two calcs
//!   tooltips (the "Dexterity" and the first "Total Base" row of CalcSections) and the
//!   SkillBuffs panels of both builds;
//! * the misc calculator (`self.calcs.getMiscCalculator`, the calculator the report calls once
//!   per node, item, jewel socket, socket group, support gem and config option) is a
//!   deterministic stand-in on both sides, as in u26: the bridge's `MiscCalc::call` boots a
//!   LuaJIT PoB per call (seconds each) and its `misc_calculator` returns the last BuildOutput's
//!   base, not a fresh one for the temporarily changed build. The stand-in's base output depends
//!   on the build's socket groups (each gem's position and name) and config input (every
//!   `configOptions` var that is set), so adding a group, a support gem or changing a config
//!   value changes it; `calcFunc(override)` adds integer amounts per node of `addNodes` (from the
//!   node's `modKey`, closure addresses normalised as u26) and per `repSlotName`/`repItem.name`.
//!   Its sums are exact, so both sides compute the same impacts (the builder's statement order,
//!   formatting and model mutations are under test, the stand-in is not);
//! * `CompareEntry` (U47) is PoB's on the Lua side; on the Rust side u46's stand-in (the entry's
//!   model from `Init_model` and a LuaCalcBridge BuildOutput);
//! * the node and item tooltips of the list's `AddValueTooltip` (U24's AddNodeTooltip, U38's
//!   AddItemTooltip) are two-line stand-ins on both sides (their own scenarios are u24/u38; the
//!   real ones would ask the misc calculator for stat differences);
//! * every other tab's `Draw` draws nothing (as u22), u22's `utf8.next`, u43's zlib rebinding.
//!
//! The virtual clock is constant within a frame, so neither side's builder yields on time
//! (`GetTime() - start > 100` never holds): the report is built in the frame its metric is
//! picked. The yield path is covered by the Rust-only `u48_progress_steps`.
//!
//! The report's list rows are compared in order: rows of equal impact keep the builder's order,
//! which follows `pairs(compareEntry.spec.allocNodes)` (power.rs ORDER note); the stand-in's
//! amounts make ties rare and the scenarios' visible rows have distinct impacts.

#![allow(non_snake_case, clippy::too_many_lines)]

use std::cell::{Cell, RefCell};
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::task::Poll;

use pob_app::app::CxApp;
use pob_app::build::init;
use pob_app::build::on_frame::{self as of, BuildTab};
use pob_app::calc::{CalcFacade, MiscCalc};
use pob_app::model::{BuildRef, Output, Override, Val};
use pob_app::tabs::compare::init::{self as ci, CompareEntryHandle, CompareEntryRef};
use pob_app::tabs::compare::power_list::ComparePowerReportListControl;
use pob_app::tabs::compare::{calcs_helpers, power, CompareTab};
use pob_host::{App, PlatformParts};
use pob_sg::trace::{FrameTrace, SideEffect};
use pob_sg::{Align, Font, Key, Sg, Sys};
use pob_ui_core::prelude::*;
use pob_ui_core::tooltip::Tooltip;
use pob_ui_oracle::boot::{self, Entry, OracleHandles};
use pob_ui_oracle::diff_report;
use pob_ui_oracle::lua_calc::LuaCalcBridge;
use pob_ui_oracle::oracle_cache::{self, LuaRun};
use pob_ui_oracle::runner::{self, Driver, LuaApp, RunResult, ScenarioApp, UserDir};


fn inputs() -> Option<PathBuf> {
    let root = pob_testpaths::require(pob_testpaths::pob_root(), "PoB checkout (.PathOfBuilding)")?;
    let dump = pob_testpaths::require(pob_testpaths::dump_dir(), "data/dump")?;
    assert!(pob_oracle::testkit::init_data(&dump), "data dump");
    let src = std::fs::canonicalize(root.join("src")).unwrap_or_else(|_| root.join("src"));
    pob_gamedata::timeless_lut::set_jewel_fs(std::sync::Arc::new(
        pob_gamedata::timeless_lut::StdJewelFs {
            script_path: src.to_string_lossy().into_owned(),
            writable: false,
        },
    ));
    Some(root)
}

fn test_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/u48")
}

/// The Lua stand-ins (see the module notes).
const LUA_STUBS: &str = r#"
local lutf8 = require("lua-utf8")
if not lutf8.next then
	local function iscont(s, p)
		local b = s:byte(p + 1) or 0
		return b >= 0x80 and b < 0xC0
	end
	lutf8.next = function(s, i, off)
		local e = #s
		local p = i - 1
		local idx = off
		if idx >= 0 then
			while p < e and idx > 0 do
				while p < e and iscont(s, p + 1) do
					p = p + 1
				end
				p = (p < e) and p + 1 or e
				idx = idx - 1
			end
		else
			while 0 < p and idx < 0 do
				local q = p
				while 0 < q and iscont(s, q - 1) do
					q = q - 1
				end
				p = (0 < q) and q - 1 or 0
				idx = idx + 1
			end
		end
		if idx ~= 0 or p == e then
			return
		end
		return p + 1, s:byte(p + 1)
	end
end
if not common.classes.CompareEntry then
	LoadModule("Classes/CompareEntry")
end
local CE = common.classes.CompareEntry
if not CE.u48Wrapped then
	local orig = CE.CompareEntry
	CE.CompareEntry = function(s, ...)
		local origSub = main.SetWindowTitleSubtext
		main.SetWindowTitleSubtext = function() end
		local ok, r = pcall(orig, s, ...)
		main.SetWindowTitleSubtext = origSub
		if not ok then error(r, 0) end
		return r
	end
	CE.u48Wrapped = true
end
local calcsHelpers = require("Classes.CompareCalcsHelpers")
local function u48hash(s)
	local x = 0
	for i = 1, #s do
		x = (x * 31 + s:byte(i)) % 1000003
	end
	return x
end
local function u48key(k)
	local s = (k or ""):gsub("function: %w+", "function")
	return s
end
local function u48misc(ct, build)
	local acc = 0
	for i, g in ipairs(build.skillsTab.socketGroupList) do
		for j, gem in ipairs(g.gemList) do
			local name = gem.grantedEffect and gem.grantedEffect.name or gem.nameSpec or "?"
			acc = acc + u48hash(i .. ":" .. j .. ":" .. name) % 997
		end
	end
	for _, varData in ipairs(ct.configOptions) do
		local v = varData.var and build.configTab.input[varData.var]
		if v ~= nil then
			acc = acc + u48hash(varData.var .. "=" .. tostring(v)) % 991
		end
	end
	local base = { Life = 4000 + acc % 5000, LifeUnreserved = 4000 + acc % 5000, Armour = 5000, Evasion = 2000,
		EnergyShield = 0, LifeRegenRecovery = 100, EnergyShieldRegenRecovery = 0, CombinedDPS = 100000 + (acc * 3) % 50000 }
	local function calcFunc(override, useFullDPS)
		local out = copyTable(base)
		for node in pairs(override.addNodes or { }) do
			local d = u48hash(u48key(node.modKey))
			out.Life = out.Life + d % 701 + 1
			out.LifeUnreserved = out.LifeUnreserved + d % 701 + 1
			out.CombinedDPS = out.CombinedDPS + d % 4999 + 1
		end
		if override.repItem then
			local d = u48hash((override.repSlotName or "") .. "|" .. override.repItem.name)
			out.Life = out.Life + d % 503 - 251
			out.LifeUnreserved = out.LifeUnreserved + d % 503 - 251
			out.CombinedDPS = out.CombinedDPS + d % 7919 - 3959
		end
		return out
	end
	return calcFunc, base
end
local function u48row(label)
	for _, sec in ipairs(LoadModule("Modules/CalcSections")) do
		for _, sub in ipairs(sec[5]) do
			for _, row in ipairs(sub.data) do
				if row.label == label then
					return row[1]
				end
			end
		end
	end
end
local b = main.modes.BUILD
local origInit = b.Init
function b:Init(...)
	origInit(self, ...)
	for _, t in ipairs({ "importTab", "notesTab", "partyTab", "configTab", "treeTab", "skillsTab", "itemsTab", "calcsTab" }) do
		if self[t] then self[t].Draw = function() end end
	end
	common.classes.PassiveTreeView.AddNodeTooltip = function(view, tooltip, node, build)
		tooltip:AddLine(16, "^7" .. node.dn)
		tooltip:AddLine(14, "^7stub " .. node.type)
	end
	if self.itemsTab then
		self.itemsTab.AddItemTooltip = function(it, tooltip, item, slot, dbMode)
			tooltip:AddLine(16, "^7" .. item.name)
			tooltip:AddLine(14, "^7stub " .. item.rarity)
		end
	end
	local ct = self.compareTab
	if not ct then return end
	ct.calcs = { getMiscCalculator = function(build) return u48misc(ct, build) end }
	ct.u48Tip2 = new("Tooltip"):Tooltip()
	ct.u48Rows = { u48row("Dexterity"), u48row("Total Base") }
	function ct:Draw(viewPort, inputEvents)
		main:DrawBackground(viewPort)
		self.controls.subTabAnchor.x = viewPort.x + 4
		self.controls.subTabAnchor.y = viewPort.y + 96
		local x, y = viewPort.x, viewPort.y + 126
		local entry = self:GetActiveCompare()
		local c = self.controls
		local mode = self.compareViewMode
		if entry and mode == "SUMMARY" then
			self:RunComparePowerReport(entry)
			c.comparePowerStatSelect.x = x + 10
			c.comparePowerStatSelect.y = y + 60
			local checkX = x + 280
			for _, k in ipairs({ "comparePowerTreeCheck", "comparePowerItemsCheck", "comparePowerGemsCheck", "comparePowerSupportGemsCheck", "comparePowerConfigCheck" }) do
				c[k].x = checkX + c[k].labelWidth
				c[k].y = y + 60
				checkX = checkX + c[k].labelWidth + 26
			end
			local listControl = c.comparePowerReportList
			if self.comparePowerCoroutine then
				listControl:SetProgress(self.comparePowerProgress)
				self.comparePowerListSynced = false
			elseif self.comparePowerResults and not self.comparePowerListSynced then
				listControl:SetReport(self.comparePowerStat, self.comparePowerResults)
				self.comparePowerListSynced = true
			elseif not self.comparePowerStat and not self.comparePowerListSynced then
				listControl:SetReport(nil, nil)
				self.comparePowerListSynced = true
			end
			if self.comparePowerStat then
				listControl.impactColumn.label = self.comparePowerStat.label or ""
			end
			listControl.x = x + 10
			listControl.y = y + 90
			listControl.width = 740
			listControl.height = 250
		end
		self:ProcessControlsInput(inputEvents, viewPort)
		if entry then
			SetDrawColor(1, 1, 1)
			local line = y + 350
			local function put(s)
				DrawString(x + 10, line, "LEFT", 12, "VAR", s)
				line = line + 14
			end
			put("u48 " .. mode .. " progress=" .. self.comparePowerProgress .. " results="
				.. (self.comparePowerResults and #self.comparePowerResults or -1))
			if mode == "SUMMARY" and self.comparePowerResults then
				local rows = { }
				for _, r in ipairs(self.comparePowerResults) do
					table.insert(rows, r.category .. "|" .. r.name .. "|" .. r.combinedImpactStr .. "|" .. tostring(r.pathDist)
						.. "|" .. tostring(r.perPointStr) .. "|" .. tostring(r.impactPercent))
				end
				table.sort(rows)
				for _, s in ipairs(rows) do
					put(s)
				end
			elseif mode == "CALCS" then
				local label = self:GetShortBuildName(self.primaryBuild.buildName)
				calcsHelpers.DrawCalcsTooltip(self.calcsTooltip, self.primaryBuild, label, self.u48Rows[1], "Dexterity", x + 10, y + 40, 200, 20, viewPort, entry)
				calcsHelpers.DrawCalcsTooltip(self.u48Tip2, self.primaryBuild, label, self.u48Rows[2], "Total Base", x + 400, y + 40, 200, 20, viewPort, entry)
				calcsHelpers.DrawSkillBreakdownPanel(self.primaryBuild, "SkillBuffs", "Buffs", x + 10, y + 300, 100, 20, viewPort)
				calcsHelpers.DrawSkillBreakdownPanel(entry, "SkillBuffs", "Buffs", x + 600, y + 300, 100, 20, viewPort)
			end
		end
		self:DrawControls(viewPort)
	end
end
"#;

/// SimpleGraphic's `Deflate` / `Inflate` (`ui_api.cpp`), bound to the port's functions (u43).
fn install_sg_zlib(lua: &mlua::Lua) -> mlua::Result<()> {
    use pob_app::tabs::import::core::{deflate, inflate};
    let g = lua.globals();
    g.set(
        "Deflate",
        lua.create_function(|lua, data: mlua::String| lua.create_string(deflate(&data.as_bytes())))?,
    )?;
    g.set(
        "Inflate",
        lua.create_function(|lua, data: mlua::String| {
            Ok(match inflate(&data.as_bytes()) {
                Some(out) => (Some(lua.create_string(out)?), None),
                None => (None, Some("data error".to_owned())),
            })
        })?,
    )?;
    Ok(())
}

// ---- the Rust stand-ins ----

thread_local! {
    /// The PoB source dir (the entries' LuaCalcBridge).
    static SRC: RefCell<Option<PathBuf>> = const { RefCell::new(None) };
    /// Drop the window titles while set (the entry constructor's, as u46).
    static DROP_TITLES: Cell<bool> = const { Cell::new(false) };
    /// `ct.u48Tip2`
    static TIP2: RefCell<Tooltip> = RefCell::new(Tooltip::default());
    /// `ct.u48Rows`
    static ROWS: RefCell<Option<(pob_app::model::Table, pob_app::model::Table)>> = const { RefCell::new(None) };
}

/// u46's `new("CompareEntry"):CompareEntry(xmlText, label)` stand-in.
fn compare_entry(cx: &mut Cx, xml: Option<&str>, label: Option<&str>) -> Option<CompareEntryRef> {
    let label = label.unwrap_or("Comparison Build").to_owned();
    let xml = xml?;
    init::flush_window_titles(cx);
    let m = pob_calc::model::Build::new(pob_gamedata::data());
    let closed = pob_calc::model::build_xml::Init_model(&m, None, Some(&label), Some(xml), false, None, false);
    DROP_TITLES.with(|d| d.set(true));
    init::flush_window_titles(cx);
    DROP_TITLES.with(|d| d.set(false));
    if closed {
        return None;
    }
    let src = SRC.with(|s| s.borrow().clone());
    let mut bridge = LuaCalcBridge::new(src);
    let _ = bridge.build_output(&m);
    Some(Rc::new(CompareEntryHandle {
        label,
        model: m,
        ext: RefCell::new(None),
    }))
}

/// The stub's `u48hash(s)`.
fn u48hash(s: &str) -> u64 {
    let mut x = 0u64;
    for &b in s.as_bytes() {
        x = (x * 31 + u64::from(b)) % 1_000_003;
    }
    x
}

/// `modKey:gsub("function: %w+", "function")` (u26's `norm_mod_key`).
fn u48key(k: &str) -> String {
    let (s, _) = pob_lua::pattern::gsub(k, "function: %w+", "function", None);
    s
}

/// Lua `tostring(v)` of a config input value.
fn tostring(v: &Val) -> String {
    match v {
        Val::Nil => "nil".to_owned(),
        Val::Bool(b) => b.to_string(),
        Val::Num(x) => pob_lua::tostring_num(*x),
        Val::Str(s) => s.to_string(),
        other => format!("{other:?}"),
    }
}

/// The stub's `u48misc(ct, build)` base output.
fn standin_base(build: &BuildRef) -> Output {
    let mut acc = 0u64;
    for (i, g) in build.skillsTab.socketGroupList.get().snapshot().iter().enumerate() {
        for (j, gem) in g.gemList.get().snapshot().iter().enumerate() {
            let name = match gem.grantedEffect.get() {
                Some(ge) => ge.name.to_string(),
                None => gem.nameSpec.get().to_string(),
            };
            acc += u48hash(&format!("{}:{}:{name}", i + 1, j + 1)) % 997;
        }
    }
    for varData in &pob_app::model::data().configOptions {
        let Some(var) = varData.var else { continue };
        let v = build.configTab.input.get(var);
        if !v.is_nil() {
            acc += u48hash(&format!("{var}={}", tostring(&v))) % 991;
        }
    }
    let t = Output::new();
    let life = 4000.0 + (acc % 5000) as f64;
    for (k, v) in [
        ("Life", life),
        ("LifeUnreserved", life),
        ("Armour", 5000.0),
        ("Evasion", 2000.0),
        ("EnergyShield", 0.0),
        ("LifeRegenRecovery", 100.0),
        ("EnergyShieldRegenRecovery", 0.0),
        ("CombinedDPS", 100_000.0 + ((acc * 3) % 50_000) as f64),
    ] {
        t.set(pob_lua::Name::from(k), v);
    }
    t
}

/// The stub's `calcFunc(override, useFullDPS)`.
struct StandinMisc {
    base: Output,
}

impl MiscCalc for StandinMisc {
    fn call(&self, ov: Option<Rc<Override>>, _use_full_dps: Option<bool>) -> Poll<Output> {
        let out = self.base.copyTable(false);
        let add = |k: &str, d: f64| out.set(pob_lua::Name::from(k), out.num(pob_lua::Name::from(k)) + d);
        if let Some(ov) = ov {
            for (_, node) in ov.addNodes.get().map(|s| s.with(pob_lua::LjIntMap::pairs)).unwrap_or_default() {
                let d = u48hash(&u48key(&node.modKey.get()));
                add("Life", (d % 701 + 1) as f64);
                add("LifeUnreserved", (d % 701 + 1) as f64);
                add("CombinedDPS", (d % 4999 + 1) as f64);
            }
            if let Some(item) = ov.repItem.get() {
                let slot = ov.repSlotName.get().map(|s| s.to_string()).unwrap_or_default();
                let d = u48hash(&format!("{slot}|{}", item.name.get()));
                add("Life", (d % 503) as f64 - 251.0);
                add("LifeUnreserved", (d % 503) as f64 - 251.0);
                add("CombinedDPS", (d % 7919) as f64 - 3959.0);
            }
        }
        Poll::Ready(out)
    }
}

/// The app's calc: U15's LuaCalcBridge for BuildOutput, the stand-in misc calculator.
struct StandinCalc {
    inner: LuaCalcBridge,
}

impl CalcFacade for StandinCalc {
    fn build_output(&mut self, build: &BuildRef) -> Poll<()> {
        self.inner.build_output(build)
    }
    fn misc_calculator(&mut self, build: &BuildRef) -> Poll<(Rc<dyn MiscCalc>, Output)> {
        let base = standin_base(build);
        Poll::Ready((Rc::new(StandinMisc { base: base.clone() }), base))
    }
}

/// `self.controls[key]` of the tab.
fn c(cx: &Cx, me: CtrlId, key: &'static str) -> CtrlId {
    cx.child(me, key).unwrap_or_else(|| panic!("controls.{key}"))
}

fn at(cx: &mut Cx, id: CtrlId, x: f64, y: f64) {
    let b = cx.base_mut(id);
    b.x = Prop::Val(x);
    b.y = Prop::Val(y);
}

fn label_width(cx: &Cx, id: CtrlId) -> f64 {
    cx.w::<pob_ui_core::controls::CheckBoxControl>(id).label_width
}

/// The stub's `u48row(label)`.
fn u48row(label: &str) -> pob_app::model::Table {
    for sec in pob_app::tabs::calcs::sections_data::load() {
        for (_, sub) in sec.sub_section.ipairs() {
            let data = sub.get(pob_lua::n!("data"));
            let Some(data) = data.table() else { continue };
            for (_, row) in data.ipairs() {
                if row.get(pob_lua::n!("label")).str().is_some_and(|l| &*l == label) {
                    return row.get(1).table().expect("row[1]");
                }
            }
        }
    }
    panic!("no CalcSections row {label}")
}

/// The stand-in `CompareTab:Draw` (the Lua stub's `ct:Draw`, statement for statement).
fn draw_compare(cx: &mut Cx, vp: &ViewPort, ev: &mut InputEvents) {
    let me = ci::tab(cx).expect("compareTab");
    pob_app::main_mode::background::draw_background(cx, vp);
    let anchor = c(cx, me, "subTabAnchor");
    at(cx, anchor, vp.x + 4.0, vp.y + 96.0);
    let (x, y) = (vp.x, vp.y + 126.0);
    let entry = CompareTab::GetActiveCompare(cx, me);
    let mode = ci::st(cx, me).compareViewMode;
    if let (Some(entry), "SUMMARY") = (&entry, mode) {
        CompareTab::RunComparePowerReport(cx, me, entry);
        at(cx, c(cx, me, "comparePowerStatSelect"), x + 10.0, y + 60.0);
        let mut checkX = x + 280.0;
        for k in [
            "comparePowerTreeCheck",
            "comparePowerItemsCheck",
            "comparePowerGemsCheck",
            "comparePowerSupportGemsCheck",
            "comparePowerConfigCheck",
        ] {
            let id = c(cx, me, k);
            let lw = label_width(cx, id);
            at(cx, id, checkX + lw, y + 60.0);
            checkX = checkX + lw + 26.0;
        }
        let list = c(cx, me, "comparePowerReportList");
        if ci::st(cx, me).comparePowerCoroutine.is_some() {
            let p = ci::st(cx, me).comparePowerProgress;
            ComparePowerReportListControl::SetProgress(cx, list, p);
            ci::st_mut(cx, me).comparePowerListSynced = false;
        } else if ci::st(cx, me).comparePowerResults.is_some() && !ci::st(cx, me).comparePowerListSynced {
            let (stat, res) = (ci::st(cx, me).comparePowerStat, power::results(cx, me));
            ComparePowerReportListControl::SetReport(cx, list, stat, res);
            ci::st_mut(cx, me).comparePowerListSynced = true;
        } else if ci::st(cx, me).comparePowerStat.is_none() && !ci::st(cx, me).comparePowerListSynced {
            ComparePowerReportListControl::SetReport(cx, list, None, None);
            ci::st_mut(cx, me).comparePowerListSynced = true;
        }
        if let Some(stat) = ci::st(cx, me).comparePowerStat {
            ComparePowerReportListControl::set_impact_label(cx, list, &stat.label);
        }
        at(cx, list, x + 10.0, y + 90.0);
        let b = cx.base_mut(list);
        b.width = Prop::Val(740.0);
        b.height = Prop::Val(250.0);
    }
    cx.process_controls_input(me, ev, vp);
    if let Some(entry) = &entry {
        cx.sg.set_draw_color(1.0, 1.0, 1.0);
        let mut lines: Vec<String> = Vec::new();
        let s = ci::st(cx, me);
        let n = power::results(cx, me).map_or(-1.0, |r| r.len() as f64);
        lines.push(format!(
            "u48 {mode} progress={} results={}",
            pob_lua::tostring_num(s.comparePowerProgress),
            pob_lua::tostring_num(n)
        ));
        let mut line = y + 350.0;
        if mode == "SUMMARY" {
            if let Some(res) = power::results(cx, me) {
                let opt = |v: Option<String>| v.unwrap_or_else(|| "nil".to_owned());
                let mut rows: Vec<String> = res
                    .iter()
                    .map(|r| {
                        format!(
                            "{}|{}|{}|{}|{}|{}",
                            r.category,
                            r.name,
                            r.combinedImpactStr,
                            opt(r.pathDist.map(pob_lua::tostring_num)),
                            opt(r.perPointStr.clone()),
                            pob_lua::tostring_num(r.impactPercent)
                        )
                    })
                    .collect();
                rows.sort_by(|a, b| a.as_bytes().cmp(b.as_bytes()));
                lines.extend(rows);
            }
            for l in lines {
                cx.sg.draw_string(x + 10.0, line, Align::Left, 12.0, Font::Var, &l);
                line += 14.0;
            }
        } else {
            for l in lines {
                cx.sg.draw_string(x + 10.0, line, Align::Left, 12.0, Font::Var, &l);
                line += 14.0;
            }
            if mode == "CALCS" {
                let label = CompareTab::GetShortBuildName(cx.build().build_name.as_deref());
                let primary = cx.build().model.clone();
                let (r1, r2) = ROWS.with(|r| r.borrow_mut().get_or_insert_with(|| (u48row("Dexterity"), u48row("Total Base"))).clone());
                let mut tt = std::mem::take(&mut ci::st_mut(cx, me).calcsTooltip);
                calcs_helpers::DrawCalcsTooltip(cx, &mut tt, &primary, &label, &r1, Some("Dexterity"), x + 10.0, y + 40.0, 200.0, 20.0, vp, entry);
                ci::st_mut(cx, me).calcsTooltip = tt;
                let mut tt2 = TIP2.with(|t| std::mem::take(&mut *t.borrow_mut()));
                calcs_helpers::DrawCalcsTooltip(cx, &mut tt2, &primary, &label, &r2, Some("Total Base"), x + 400.0, y + 40.0, 200.0, 20.0, vp, entry);
                TIP2.with(|t| *t.borrow_mut() = tt2);
                calcs_helpers::DrawSkillBreakdownPanel(cx, &primary, "SkillBuffs", Some("Buffs"), x + 10.0, y + 300.0, 100.0, 20.0, vp);
                calcs_helpers::DrawSkillBreakdownPanel(cx, &entry.model, "SkillBuffs", Some("Buffs"), x + 600.0, y + 300.0, 100.0, 20.0, vp);
            }
        }
    }
    cx.draw_controls(me, vp, None);
}

fn rust_stubs() {
    TIP2.with(|t| *t.borrow_mut() = Tooltip::default());
    ROWS.with(|r| *r.borrow_mut() = None);
    init::set_hooks(|h| *h = init::default_hooks());
    ci::set_compare_hooks(|h| {
        *h = ci::default_compare_hooks();
        h.new_compare_entry = Rc::new(compare_entry);
    });
    pob_app::tree_view::set_hooks(|h| {
        *h = pob_app::tree_view::default_hooks();
        h.add_node_tooltip = Rc::new(|cx, _v, tt, node| {
            tt.add_line(cx, 16.0, &format!("^7{}", node.dn.get()));
            tt.add_line(cx, 14.0, &format!("^7stub {}", node.r#type.get()));
        });
    });
    pob_app::tabs::items::slot_helper::set_items_hooks(|h| {
        *h = pob_app::tabs::items::slot_helper::default_items_hooks();
        h.add_item_tooltip = Rc::new(|cx, tt, item, _slot, _db| {
            tt.add_line(cx, 16.0, &format!("^7{}", item.name.get()));
            tt.add_line(cx, 14.0, &format!("^7stub {}", item.rarity.get()));
        });
    });
    of::set_shell_hooks(|h| {
        *h = of::default_shell_hooks();
        let all_tabs = h.draw_tab.clone();
        h.draw_tab = Rc::new(move |cx, tab, vp, ev| match tab {
            BuildTab::Compare => draw_compare(cx, vp, ev),
            // LUA_STUBS replaces the other tabs' Draw
            BuildTab::Notes | BuildTab::Party => {}
            BuildTab::Tree
            | BuildTab::Skills
            | BuildTab::Items
            | BuildTab::Calcs
            | BuildTab::Config
            | BuildTab::Import => {}
            #[allow(unreachable_patterns)]
            _ => all_tabs(cx, tab, vp, ev),
        });
    });
}

/// `#! NAME ARG` directives.
fn directives(text: &str, name: &str) -> Vec<String> {
    let tag = format!("#! {name}");
    text.lines()
        .filter_map(|l| l.trim().strip_prefix(tag.as_str()))
        .map(|s| s.trim().to_owned())
        .collect()
}

// ---- the Lua side ----

struct LuaSide {
    app: LuaApp,
}

impl App for LuaSide {
    fn sg(&mut self) -> &mut Sg {
        self.app.sg()
    }
    fn sys(&mut self) -> &mut Sys {
        self.app.sys()
    }
    fn on_init(&mut self) {
        self.app.on_init();
        self.app
            .with_vm(|pob| {
                pob.eval::<()>(LUA_STUBS)?;
                install_sg_zlib(&pob.lua)
            })
            .unwrap_or_else(|e| panic!("Lua stubs: {e}"));
    }
    fn on_frame(&mut self) {
        self.app.on_frame();
    }
    fn on_key_down(&mut self, key: Key, double_click: bool) {
        self.app.on_key_down(key, double_click);
    }
    fn on_key_up(&mut self, key: Key) {
        self.app.on_key_up(key);
    }
    fn on_char(&mut self, ch: Key) {
        self.app.on_char(ch);
    }
    fn on_lua_error(&mut self, msg: String) {
        self.app.on_lua_error(msg);
    }
}

impl ScenarioApp for LuaSide {
    fn load_build(&mut self, name: &str, xml: &str) -> anyhow::Result<()> {
        self.app.load_build(name, xml)
    }
    fn snapshot(&mut self) -> anyhow::Result<Option<String>> {
        self.app.snapshot()
    }
    fn after_frame(&mut self) -> anyhow::Result<()> {
        self.app.after_frame()
    }
    fn take_error(&mut self) -> Option<String> {
        self.app.take_error()
    }
}

// ---- the Rust side ----

/// Records the platform side effects the Lua side's bridge records (as in u16-u46).
struct RecordingPlatform {
    inner: Box<dyn pob_sg::Platform>,
    effects: Rc<RefCell<Vec<SideEffect>>>,
}

impl pob_sg::Platform for RecordingPlatform {
    fn time_ms(&self) -> i64 {
        self.inner.time_ms()
    }
    fn copy(&mut self, text: &str) {
        self.effects.borrow_mut().push(SideEffect::Copy(text.to_owned()));
        self.inner.copy(text);
    }
    fn paste(&mut self) -> Option<String> {
        self.inner.paste()
    }
    fn open_url(&mut self, url: &str) {
        self.effects.borrow_mut().push(SideEffect::OpenUrl(url.to_owned()));
        self.inner.open_url(url);
    }
    fn set_window_title(&mut self, title: &str) {
        if DROP_TITLES.with(Cell::get) {
            return;
        }
        self.effects
            .borrow_mut()
            .push(SideEffect::WindowTitle(title.to_owned()));
        self.inner.set_window_title(title);
    }
    fn take_screenshot(&mut self) {
        self.effects.borrow_mut().push(SideEffect::Screenshot);
        self.inner.take_screenshot();
    }
    fn restart(&mut self) {
        self.inner.restart();
    }
    fn exit(&mut self, msg: Option<&str>) {
        self.effects.borrow_mut().push(SideEffect::Exit);
        self.inner.exit(msg);
    }
    fn script_path(&self) -> String {
        self.inner.script_path()
    }
    fn runtime_path(&self) -> String {
        self.inner.runtime_path()
    }
    fn user_path(&self) -> Result<String, (String, String)> {
        self.inner.user_path()
    }
    fn con_print(&mut self, s: &str) {
        self.inner.con_print(s);
    }
}

struct NullPlatform;
impl pob_sg::Platform for NullPlatform {
    fn time_ms(&self) -> i64 {
        0
    }
    fn copy(&mut self, _: &str) {}
    fn paste(&mut self) -> Option<String> {
        None
    }
    fn open_url(&mut self, _: &str) {}
    fn set_window_title(&mut self, _: &str) {}
    fn take_screenshot(&mut self) {}
    fn restart(&mut self) {}
    fn exit(&mut self, _: Option<&str>) {}
    fn script_path(&self) -> String {
        String::new()
    }
    fn runtime_path(&self) -> String {
        String::new()
    }
    fn user_path(&self) -> Result<String, (String, String)> {
        Ok(String::new())
    }
    fn con_print(&mut self, _: &str) {}
}

struct RustSide {
    app: pob_app::PobApp,
    effects: Rc<RefCell<Vec<SideEffect>>>,
}

impl RustSide {
    fn new(mut parts: PlatformParts, src: PathBuf) -> RustSide {
        let effects = Rc::new(RefCell::new(Vec::new()));
        let inner = std::mem::replace(&mut parts.sys.platform, Box::new(NullPlatform));
        parts.sys.platform = Box::new(RecordingPlatform {
            inner,
            effects: effects.clone(),
        });
        SRC.with(|s| *s.borrow_mut() = Some(src.clone()));
        let mut app = pob_app::PobApp::with_calc(
            parts,
            Rc::new(move || Box::new(StandinCalc { inner: LuaCalcBridge::new(Some(src.clone())) })),
        );
        effects.borrow_mut().clear(); // Launch.lua:11 runs at script load, before tracing
                                      // pob-ui-oracle patch 4's equivalent (CCR-0091)
        app.cx.app_mut().launch.force_release_install = true;
        RustSide { app, effects }
    }
    fn flush(&mut self) {
        let e: Vec<_> = self.effects.borrow_mut().drain(..).collect();
        if let Some(t) = self.app.cx.sg.trace.as_mut() {
            for e in e {
                t.effect(e);
            }
        }
    }
}

impl App for RustSide {
    fn sg(&mut self) -> &mut Sg {
        self.app.sg()
    }
    fn sys(&mut self) -> &mut Sys {
        self.app.sys()
    }
    fn on_init(&mut self) {
        rust_stubs();
        self.app.on_init();
        self.flush();
    }
    fn on_frame(&mut self) {
        self.app.on_frame();
        self.flush();
    }
    fn on_key_down(&mut self, key: Key, double_click: bool) {
        self.app.on_key_down(key, double_click);
        self.flush();
    }
    fn on_key_up(&mut self, key: Key) {
        self.app.on_key_up(key);
        self.flush();
    }
    fn on_char(&mut self, ch: Key) {
        self.app.on_char(ch);
        self.flush();
    }
    fn on_lua_error(&mut self, msg: String) {
        self.app.on_lua_error(msg);
    }
}

impl ScenarioApp for RustSide {
    fn load_build(&mut self, name: &str, xml: &str) -> anyhow::Result<()> {
        self.app.load_build(name, xml);
        Ok(())
    }
    fn snapshot(&mut self) -> anyhow::Result<Option<String>> {
        Ok(self.app.snapshot())
    }
    fn after_frame(&mut self) -> anyhow::Result<()> {
        if let Some(p) = self
            .app
            .cx
            .app()
            .launch
            .prompt_msg
            .clone()
            .filter(|p| p.starts_with("^1Error"))
        {
            anyhow::bail!("the error prompt (launch:ShowErrMsg): {p}");
        }
        Ok(())
    }
}


// ---- runs ----

// ---- runs ----

fn drop_image_loads(frames: &mut [FrameTrace]) {
    for f in frames {
        f.effects.retain(|e| !matches!(e, SideEffect::ImageLoad(_)));
    }
}

struct Side {
    frames: Vec<FrameTrace>,
    error: Option<String>,
    snapshots: Vec<(String, String)>,
}

/// The Lua side of one run through the Lua-side cache (TP4, as u41): PoB boot, `script` and what
/// the test keeps of the VM and the user dir afterwards (`extra`, JSON). The key covers this file
/// (stubs and Lua-side wrapper), the Lua-side code, PoB, this test's scripts and fixtures, the
/// fixture user dir, the script, the screen and the other arguments.
fn lua_run(
    root: &Path,
    script: &pob_host::script::Script,
    user: &UserDir,
    fixture: Option<&Path>,
    screen: pob_sg::ScreenInfo,
) -> anyhow::Result<LuaRun> {
    let scripts = test_dir();
    let mut inputs: Vec<&Path> = vec![&scripts];
    inputs.extend(fixture);
    let key = oracle_cache::ui_key(
        "ui-u48",
        pob_ui_oracle::test_source!(),
        root,
        &inputs,
        &[&format!("{script:?}"), &format!("{screen:?}")],
    );
    oracle_cache::cached_lua_run(key, || {
        let handles = OracleHandles::default();
        let lua_app = runner::lua_side(root, user, &handles, Entry::Launch, false)?;
        let mut lua_side = LuaSide { app: lua_app };
        let mut d = Driver::new(screen, handles, user.path.clone());
        let (mut frames, r) = runner::drive(&mut lua_side, script, &mut d);
        drop(lua_side);
        drop_image_loads(&mut frames);
        Ok(LuaRun {
            frames,
            error: r.err().map(|e| e.to_string()),
            snapshots: std::mem::take(&mut d.snapshots),
            extra: Vec::new(),
        })
    })
}

/// Run `script` on both sides with `fixture` as the user dir (fresh copies per side). The Lua
/// VM is dropped before the Rust side starts (memory).
fn run_sides(
    root: &Path,
    script: &pob_host::script::Script,
    fixture: Option<&Path>,
    screen: pob_sg::ScreenInfo,
) -> anyhow::Result<(Side, Side)> {
    let user = UserDir::new(fixture)?;

    // Lua (cached, TP4)
    let run = lua_run(root, script, &user, fixture, screen)?;
    let lua = Side {
        frames: run.frames,
        error: run.error,
        snapshots: run.snapshots,
    };

    // Rust
    user.reset()?;
    let handles = OracleHandles::default();
    let (sg, sys) = boot::oracle_parts(root, &user.path, &handles)?;
    let src = std::fs::canonicalize(root.join("src")).unwrap_or_else(|_| root.join("src"));
    let mut rust_side = RustSide::new(PlatformParts { sg, sys }, src);
    let mut d = Driver::new(screen, handles, user.path.clone());
    let res = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| runner::drive(&mut rust_side, script, &mut d)));
    let (mut frames, error) = match res {
        Ok((frames, r)) => (frames, r.err().map(|e| e.to_string())),
        Err(p) => (Vec::new(), Some(format!("panicked: {}", pob_host::panic_message(&*p)))),
    };
    drop(rust_side);
    drop_image_loads(&mut frames);
    let rust = Side {
        frames,
        error,
        snapshots: std::mem::take(&mut d.snapshots),
    };
    Ok((lua, rust))
}


fn out_dir() -> PathBuf {
    std::env::var_os("CARGO_TARGET_DIR")
        .map_or_else(|| PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target"), PathBuf::from)
        .join("ui-parity")
}

fn draw_ops(frames: &[FrameTrace]) -> usize {
    use pob_sg::trace::ApiCall;
    frames
        .iter()
        .flat_map(|f| &f.calls)
        .filter(|c| matches!(c.call, ApiCall::DrawImage { .. } | ApiCall::DrawImageQuad { .. } | ApiCall::DrawString { .. }))
        .count()
}

/// The texts of a run's DrawString calls (for `#! u46-expect-text`).
fn draw_texts(frames: &[FrameTrace]) -> Vec<String> {
    use pob_sg::trace::ApiCall;
    frames
        .iter()
        .flat_map(|f| &f.calls)
        .filter_map(|c| match &c.call {
            ApiCall::DrawString { text, .. } => Some(text.to_string()),
            _ => None,
        })
        .collect()
}

fn window_titles(frames: &[FrameTrace]) -> Vec<String> {
    frames
        .iter()
        .flat_map(|f| &f.effects)
        .filter_map(|e| match e {
            SideEffect::WindowTitle(t) => Some(t.clone()),
            _ => None,
        })
        .collect()
}

fn distinct_titles(t: &[String]) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for s in t {
        if out.last() != Some(s) {
            out.push(s.clone());
        }
    }
    out
}


/// `#! u48-expect-text TEXT`: some Lua DrawString contains TEXT (guards a script whose clicks
/// silently stopped reaching the tab, which both sides would agree on).
fn check_expectations(text: &str, lua: &Side) -> Vec<String> {
    let mut errs = Vec::new();
    let texts = draw_texts(&lua.frames);
    for want in directives(text, "u48-expect-text") {
        if !texts.iter().any(|t| t.contains(want.as_str())) {
            errs.push(format!("no Lua DrawString contains {want:?}"));
        }
    }
    errs
}

/// One case's outcome (TP4: cases run on `oracle_cache::par_map`; collected in case order).
#[derive(Default)]
struct CaseOut {
    lines: Vec<String>,
    failures: Vec<String>,
    ran: usize,
}

#[test]
fn u48_scenarios() {
    let Some(root) = inputs() else { return };
    let mut scripts: Vec<PathBuf> = std::fs::read_dir(test_dir())
        .expect("tests/u48")
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|x| x == "pobscript"))
        .collect();
    scripts.sort();
    assert!(!scripts.is_empty(), "no tests/u48/*.pobscript");
    let filter = std::env::var("U48_FILTER").ok();
    let mut cases = Vec::new();
    for path in scripts {
        let stem = path.file_stem().unwrap().to_string_lossy().into_owned();
        if filter.as_ref().is_some_and(|f| !stem.contains(f.as_str())) {
            continue;
        }
        cases.push((path.clone(), stem.clone()));
    }
    let results = oracle_cache::par_map(cases, |(path, stem)| {
        let mut out = CaseOut::default();
        let text = std::fs::read_to_string(&path).unwrap();
        let (script, base_dir) = runner::load_script(&path).unwrap();
        let fx = runner::fixtures(&script, &base_dir).unwrap();
        let tag = format!("u48_{stem}");
        let (lua, rust) = match run_sides(&root, &script, fx.user_dir.as_deref(), runner::screen(1280, 800, 1.0)) {
            Ok(s) => s,
            Err(e) => {
                out.failures.push(format!("{tag}: {e:#}"));
                return out;
            }
        };
        out.ran += 1;
        let titles = (window_titles(&lua.frames), window_titles(&rust.frames));
        let expect = check_expectations(&text, &lua);
        if std::env::var_os("U48_DUMP_TEXT").is_some() {
            // the Lua side's distinct DrawString texts, in first-drawn order (for writing scripts)
            let mut seen = std::collections::HashSet::new();
            let texts: Vec<String> = draw_texts(&lua.frames).into_iter().filter(|t| seen.insert(t.clone())).collect();
            let dir = out_dir().join(&tag);
            let _ = std::fs::create_dir_all(&dir);
            let _ = std::fs::write(dir.join("lua_texts.txt"), texts.join("\n"));
        }
        let run = RunResult {
            lua: lua.frames,
            rust: rust.frames,
            rust_error: rust.error.clone(),
            lua_snapshots: lua.snapshots.clone(),
            rust_snapshots: rust.snapshots.clone(),
            audit: Vec::new(),
            compared: true,
        };
        let n = diff_report::write_report(&out_dir(), &tag, &run, false).unwrap_or(usize::MAX);
        let ops = draw_ops(&run.lua);
        out.lines.push(format!(
            "{tag}: {} frames, {ops} Lua draw ops, {n} divergences, titles {:?}{}{}{}",
            run.lua.len(),
            distinct_titles(&titles.0),
            lua.error.as_ref().map_or_else(String::new, |e| format!(", Lua error: {e}")),
            rust.error.as_ref().map_or_else(String::new, |e| format!(", Rust error: {e}")),
            if expect.is_empty() {
                String::new()
            } else {
                format!(", expectations: {}", expect.join("; "))
            }
        ));
        if n != 0
            || rust.error.is_some()
            || lua.error.is_some()
            || run.lua.len() != run.rust.len()
            || ops == 0
            || titles.0 != titles.1
            || !expect.is_empty()
        {
            out.failures.push(format!(
                "{tag}: {n} divergences, frames lua {} rust {}, {ops} ops, titles lua {:?} rust {:?}, errors lua {:?} rust {:?}, expectations {expect:?} (see {})",
                run.lua.len(),
                run.rust.len(),
                distinct_titles(&titles.0),
                distinct_titles(&titles.1),
                lua.error,
                rust.error,
                out_dir().join(&tag).join("report.txt").display()
            ));
        }

        out
    });
    let mut failures = Vec::new();
    let mut ran = 0;
    for case in results {
        for l in &case.lines {
            println!("{l}");
        }
        failures.extend(case.failures);
        ran += case.ran;
    }
    println!("u48: {ran} runs");
    println!("{}", oracle_cache::summary());
    assert!(ran > 0 || filter.is_some(), "no run");
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// A platform whose clock passes `step` ms on every read (`GetTime()` inside one resume).
struct TickingPlatform {
    inner: Box<dyn pob_sg::Platform>,
    now: Cell<i64>,
    step: i64,
}

impl pob_sg::Platform for TickingPlatform {
    fn time_ms(&self) -> i64 {
        self.now.set(self.now.get() + self.step);
        self.now.get()
    }
    fn copy(&mut self, text: &str) {
        self.inner.copy(text);
    }
    fn paste(&mut self) -> Option<String> {
        self.inner.paste()
    }
    fn open_url(&mut self, url: &str) {
        self.inner.open_url(url);
    }
    fn set_window_title(&mut self, title: &str) {
        self.inner.set_window_title(title);
    }
    fn take_screenshot(&mut self) {
        self.inner.take_screenshot();
    }
    fn restart(&mut self) {
        self.inner.restart();
    }
    fn exit(&mut self, msg: Option<&str>) {
        self.inner.exit(msg);
    }
    fn script_path(&self) -> String {
        self.inner.script_path()
    }
    fn runtime_path(&self) -> String {
        self.inner.runtime_path()
    }
    fn user_path(&self) -> Result<String, (String, String)> {
        self.inner.user_path()
    }
    fn con_print(&mut self, s: &str) {
        self.inner.con_print(s);
    }
}

/// The report's result lines (the stand-in Draw's sorted status lines).
fn result_lines(cx: &Cx, me: CtrlId) -> Vec<String> {
    let mut rows: Vec<String> = power::results(cx, me)
        .map(|res| {
            res.iter()
                .map(|r| {
                    format!(
                        "{}|{}|{}|{:?}|{:?}|{}",
                        r.category,
                        r.name,
                        r.combinedImpactStr,
                        r.pathDist,
                        r.perPointStr,
                        r.impactPercent
                    )
                })
                .collect()
        })
        .unwrap_or_default();
    rows.sort();
    rows
}

/// The yield path (Rust only, no oracle): with a clock that passes 60 ms per read,
/// `ComparePowerBuilder` yields on time (`coroutine.running() and GetTime() - start > 100`), so
/// `RunComparePowerReport` resumes it over many steps, `comparePowerProgress` rises, the list
/// shows "Calculating... N%" (SetProgress), and the results equal the one-resume run's.
#[test]
fn u48_progress_steps() {
    // TP4: the process-wide case pool (shared with the `par_map` cases) instead of the lock
    let _permit = oracle_cache::Permit::acquire();
    let Some(root) = inputs() else { return };
    // a_full_dps's steps up to the import (no metric picked)
    let script = runner::parse_script(
        "screen 1280 800 dpi 1\nframes 2\nload-build \"Builds/allflame_exp_001.xml\"\nframes 3\nmove 116 122\nclick LEFTBUTTON\nframes 3\nmove 718 48\nclick LEFTBUTTON\nframes 2\nmove 640 460\nclick LEFTBUTTON\nframes 2\nmove 640 236\nclick LEFTBUTTON\nframes 1\npress RETURN\nframes 3\nmove 640 236\nclick LEFTBUTTON\nframes 2\nmove 585 625\nclick LEFTBUTTON\nframes 3\n",
    )
    .unwrap();
    let user = UserDir::new(Some(&test_dir().join("user"))).unwrap();
    let handles = OracleHandles::default();
    let (sg, sys) = boot::oracle_parts(&root, &user.path, &handles).unwrap();
    let src = std::fs::canonicalize(root.join("src")).unwrap_or_else(|_| root.join("src"));
    let mut rust_side = RustSide::new(PlatformParts { sg, sys }, src);
    let mut d = Driver::new(runner::screen(1280, 800, 1.0), handles, user.path.clone());
    let (_, r) = runner::drive(&mut rust_side, &script, &mut d);
    r.unwrap();
    let cx = &mut rust_side.app.cx;
    let me = ci::tab(cx).expect("compareTab");
    let entry = CompareTab::GetActiveCompare(cx, me).expect("the imported comparison build");
    let full_dps = pob_app::model::data()
        .powerStatList
        .stats
        .iter()
        .find(|s| s.stat.is_some_and(|n| n == pob_lua::n!("FullDPS")))
        .expect("Full DPS");

    // one resume (the oracle clock does not move within a frame)
    ci::st_mut(cx, me).comparePowerStat = Some(full_dps);
    ci::st_mut(cx, me).comparePowerDirty = true;
    CompareTab::RunComparePowerReport(cx, me, &entry);
    assert!(ci::st(cx, me).comparePowerCoroutine.is_none(), "one resume builds the report");
    let one = result_lines(cx, me);
    assert!(!one.is_empty(), "the report has rows");

    // a clock that passes 60 ms per read: the builder yields after (almost) every item
    let inner = std::mem::replace(&mut cx.sys.platform, Box::new(NullPlatform));
    cx.sys.platform = Box::new(TickingPlatform {
        inner,
        now: Cell::new(1_000_000),
        step: 60,
    });
    let list = c(cx, me, "comparePowerReportList");
    ci::st_mut(cx, me).comparePowerDirty = true;
    let mut steps = 0;
    let mut progress: Vec<f64> = Vec::new();
    let mut statuses: Vec<String> = Vec::new();
    loop {
        CompareTab::RunComparePowerReport(cx, me, &entry);
        steps += 1;
        if ci::st(cx, me).comparePowerCoroutine.is_none() {
            break;
        }
        assert!(pob_ui_core::coroutine::co::any_alive(), "step {steps}");
        assert!(power::results(cx, me).is_none(), "no results while running (step {steps})");
        let p = ci::st(cx, me).comparePowerProgress;
        if progress.last() != Some(&p) {
            progress.push(p);
        }
        ComparePowerReportListControl::SetProgress(cx, list, p);
        let s = cx.w::<ComparePowerReportListControl>(list).status_text.clone().unwrap_or_default();
        if statuses.last() != Some(&s) {
            statuses.push(s);
        }
        assert!(steps < 100_000, "the report never ends");
    }
    let stepped = result_lines(cx, me);
    println!(
        "u48_progress_steps: {} rows, {steps} resumes, {} distinct progress values (first {:?}, last {:?}), statuses {:?}..{:?}",
        stepped.len(),
        progress.len(),
        progress.first(),
        progress.last(),
        statuses.first(),
        statuses.last()
    );
    assert!(steps > 2, "the builder yields on time ({steps} resumes)");
    assert!(progress.windows(2).all(|w| w[0] <= w[1]), "progress rises: {progress:?}");
    assert!(statuses.first().is_some_and(|s| s.starts_with("Calculating... ")), "{statuses:?}");
    assert_eq!(ci::st(cx, me).comparePowerProgress, 100.0);
    assert_eq!(one, stepped, "the stepped report equals the one-resume report");
    drop(rust_side);
}

