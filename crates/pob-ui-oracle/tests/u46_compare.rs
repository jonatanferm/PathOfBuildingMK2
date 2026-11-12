//! U46 acceptance (ui doc §12 U46 row: CompareTab A, CompareTab.lua:1-1649; "scenarios per popup
//! and control cluster"): the full app (Launch, the build list, the build shell, the Compare tab
//! built by U46's constructor) on both sides of the UI oracle, in u22's form (the wave-4
//! template). Owner: U46.
//!
//! `u46_scenarios`: every `tests/u46/*.pobscript` at 1280x800 @1.0 and 1920x1080 @1.5 (both 1280
//! virtual px wide), or once at a script's own `screen`. Each script opens `allflame_exp_001`
//! from the build list of `tests/u46/user`, presses the Compare button and drives the tab's
//! controls and popups. Per run: zero T3 divergences (draw ops per `(layer, sub)` bucket and
//! side effects, frame by frame), the same window titles, and T4 at every `snapshot` step
//! (u22's normalisation).
//!
//! The Rust side's calc is U15's LuaCalcBridge (as u20-u22, u45).
//!
//! What runs on each side:
//! * the CompareTab constructor and InitControls, GetShortBuildName, PopulateSetDropdown,
//!   RebuildConfigControls/makeConfigControl, CopyCompareConfig, ImportBuild, RemoveBuild,
//!   UpdateBuildSelector, GetActiveCompare, CopyCompareSpecToPrimary, GetJewelComparisonSlots,
//!   CopyCompareItemToPrimary, OpenImportPopup and OpenImportFolderPopup (with its BuildListControl
//!   overrides): PoB's (Lua) and U46's (Rust);
//! * `CompareTab:Draw` (U47's, CompareTab.lua:1650-1870) is a stand-in on both sides
//!   ([`LUA_STUBS`] and [`draw_compare`]): Draw's top-bar lines (background, the sub-tab anchor
//!   position, the dividers, `ProcessControlsInput`, the "No comparison build loaded" text,
//!   `DrawControls`), the set-list half of `UpdateSetSelectors` (the three PopulateSetDropdown
//!   calls, U46's), the `RebuildConfigControls` trigger of `LayoutConfigView`, fixed positions
//!   for the mode controls that U47's layout functions place (so they draw inside the content
//!   area), and status lines that print U46's state: the mode, the entries, the config controls
//!   per section with each one's initial value (makeConfigControl), and the
//!   GetJewelComparisonSlots records in the Items view. Two test buttons (`u46CopyItem`,
//!   `u46Short`) call CopyCompareItemToPrimary("Ring 1", entry, true) and print
//!   GetShortBuildName, whose callers (U49) are not ported;
//! * `CompareEntry` (U47) is PoB's on the Lua side; on the Rust side [`compare_entry`] builds the
//!   entry's model with calc:BM-A's `Init_model` and runs a LuaCalcBridge `BuildOutput` on it.
//!   The window titles CompareEntry's own tab constructors send are not compared (both sides
//!   drop them: the Lua stub turns `main:SetWindowTitleSubtext` off while the constructor runs,
//!   the Rust side drops the titles the entry's model queued); the title ImportBuild restores is;
//! * `ComparePowerReportListControl` (U48) draws nothing on both sides (the Lua instance's `Draw`
//!   is replaced; the Rust side installs U46's placeholder control, which has an empty `Draw`,
//!   instead of U48's control: CCR-0138 §4);
//! * every other tab's `Draw` draws nothing (as u22), u22's `utf8.next`, and u43's
//!   `Deflate`/`Inflate` rebinding of the Lua globals (the oracle's natives return "" for a
//!   failed stream, so an invalid code would read as valid).

#![allow(non_snake_case, clippy::too_many_lines)]

use std::cell::{Cell, RefCell};
use std::path::{Path, PathBuf};
use std::rc::Rc;

use pob_app::app::CxApp;
use pob_app::build::init;
use pob_app::build::on_frame::{self as of, BuildTab, TabFlag};
use pob_app::calc::CalcFacade;
use pob_app::tabs::compare::init::{self as ci, CompareEntryHandle, CompareEntryRef, SetTab};
use pob_app::tabs::compare::CompareTab;
use pob_calc::model::{XmlChild, XmlNode};
use pob_host::{App, PlatformParts};
use pob_sg::trace::{FrameTrace, SideEffect};
use pob_sg::{Align, Font, Key, Sg, Sys};
use pob_ui_core::controls::{ButtonControl, CheckBoxControl, DropDownControl, EditControl};
use pob_ui_core::prelude::*;
use pob_ui_oracle::boot::{self, Entry, OracleHandles};
use pob_ui_oracle::diff_report;
use pob_ui_oracle::lua_calc::LuaCalcBridge;
use pob_ui_oracle::oracle_cache::{self, LuaRun};
use pob_ui_oracle::runner::{self, Driver, LuaApp, RunResult, ScenarioApp, UserDir};


const SCREENS: [(i32, i32, f32); 2] = [(1280, 800, 1.0), (1920, 1080, 1.5)];

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
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/u46")
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
if not CE.u46Wrapped then
	local orig = CE.CompareEntry
	CE.CompareEntry = function(s, ...)
		local origSub = main.SetWindowTitleSubtext
		main.SetWindowTitleSubtext = function() end
		local ok, r = pcall(orig, s, ...)
		main.SetWindowTitleSubtext = origSub
		if not ok then error(r, 0) end
		return r
	end
	CE.u46Wrapped = true
end
local function u46val(varData, ctrl)
	if varData.type == "check" then
		return tostring(ctrl.state)
	elseif varData.type == "list" then
		return tostring(ctrl.selIndex)
	end
	return ctrl.buf
end
local b = main.modes.BUILD
local origInit = b.Init
function b:Init(...)
	origInit(self, ...)
	for _, t in ipairs({ "importTab", "notesTab", "partyTab", "configTab", "treeTab", "skillsTab", "itemsTab", "calcsTab" }) do
		if self[t] then self[t].Draw = function() end end
	end
	local ct = self.compareTab
	if not ct then return end
	ct.controls.comparePowerReportList.Draw = function() end
	ct.controls.u46CopyItem = new("ButtonControl"):ButtonControl(nil, {0, 0, 120, 20}, "u46 copy ring", function()
		local entry = ct:GetActiveCompare()
		if entry then
			ct:CopyCompareItemToPrimary("Ring 1", entry, true)
		end
	end)
	ct.controls.u46CopyItem.shown = function()
		return ct.compareViewMode == "ITEMS" and ct:GetActiveCompare() ~= nil
	end
	function ct:Draw(viewPort, inputEvents)
		main:DrawBackground(viewPort)
		self.controls.subTabAnchor.x = viewPort.x + 4
		self.controls.subTabAnchor.y = viewPort.y + 96
		if #self.compareEntries > 0 then
			SetDrawColor(0.25, 0.25, 0.25)
			DrawImage(nil, viewPort.x + 4, viewPort.y + 32, viewPort.width - 8, 2)
			DrawImage(nil, viewPort.x + 4, viewPort.y + 88, viewPort.width - 8, 2)
			DrawImage(nil, viewPort.x + 4, viewPort.y + 122, viewPort.width - 8, 2)
		end
		local x, y, w, h = viewPort.x, viewPort.y + 126, viewPort.width, viewPort.height - 126
		local entry = self:GetActiveCompare()
		local c = self.controls
		local function at(ctrl, cx, cy)
			ctrl.x = cx
			ctrl.y = cy
		end
		local mode = self.compareViewMode
		if entry then
			self:PopulateSetDropdown(entry.skillsTab, "skillSetOrderList", "skillSets", "activeSkillSetId", c.compareSkillSetSelect)
			self:PopulateSetDropdown(entry.itemsTab, "itemSetOrderList", "itemSets", "activeItemSetId", c.compareItemSetSelect)
			self:PopulateSetDropdown(entry.configTab, "configSetOrderList", "configSets", "activeConfigSetId", c.compareConfigSetSelect)
			if mode == "CONFIG" then
				if self.configCompareId ~= self.activeCompareIndex or self.configNeedsRebuild then
					self:RebuildConfigControls(entry)
					self.configCompareId = self.activeCompareIndex
					self.configNeedsRebuild = false
				end
				at(c.copyConfigBtn, x + 315, y + 4)
				at(c.configToggleBtn, x + 315, y + 28)
				at(c.configSearchEdit, x + 10, y + 28)
				at(c.configPrimarySetLabel, x + 10, y + 6)
				at(c.configPrimarySetSelect, x + 80, y + 4)
			elseif mode == "TREE" then
				at(c.leftSpecSelect, x + 10, y + 30)
				at(c.rightSpecSelect, x + 420, y + 30)
				at(c.treeOverlayCheck, x + 10 + c.treeOverlayCheck.labelWidth, y + 56)
				at(c.overlayTreeSearch, x + 200, y + 56)
				at(c.leftFooterAnchor, x + 10, y + 80)
				at(c.rightFooterAnchor, x + 420, y + 80)
			elseif mode == "ITEMS" then
				at(c.itemsExpandedCheck, x + 10 + c.itemsExpandedCheck.labelWidth, y + 8)
				at(c.primaryItemSetLabel, x + 10, y + 54)
				at(c.primaryItemSetSelect, x + 80, y + 52)
				at(c.compareItemSetLabel2, x + 420, y + 54)
				at(c.compareItemSetSelect2, x + 490, y + 52)
				at(c.primaryTreeSetLabel, x + 10, y + 80)
				at(c.primaryTreeSetSelect, x + 80, y + 78)
				at(c.compareTreeSetLabel, x + 420, y + 80)
				at(c.compareTreeSetSelect, x + 490, y + 78)
				at(c.u46CopyItem, x + 200, y + 8)
			elseif mode == "SUMMARY" then
				at(c.comparePowerStatSelect, x + 10, y + 60)
				at(c.comparePowerTreeCheck, x + 250 + c.comparePowerTreeCheck.labelWidth, y + 61)
				at(c.comparePowerItemsCheck, x + 330 + c.comparePowerItemsCheck.labelWidth, y + 61)
				at(c.comparePowerGemsCheck, x + 410 + c.comparePowerGemsCheck.labelWidth, y + 61)
				at(c.comparePowerSupportGemsCheck, x + 520 + c.comparePowerSupportGemsCheck.labelWidth, y + 61)
				at(c.comparePowerConfigCheck, x + 650 + c.comparePowerConfigCheck.labelWidth, y + 61)
				at(c.comparePowerReportList, x + 10, y + 90)
			elseif mode == "CALCS" then
				at(c.calcsShowOnlyDifferencesCheck, x + 10 + c.calcsShowOnlyDifferencesCheck.labelWidth, y + 8)
			end
		end
		self:ProcessControlsInput(inputEvents, viewPort)
		if not entry then
			SetViewport(x, y, w, h)
			SetDrawColor(1, 1, 1)
			DrawString(0, 40, "CENTER", 20, "VAR",
				"^7No comparison build loaded.")
			DrawString(0, 70, "CENTER", 16, "VAR",
				"^7Click " .. colorCodes.POSITIVE .. "Import..." .. "^7 above to import a build to compare against.")
			SetViewport()
		else
			SetDrawColor(1, 1, 1)
			local line = y + 110
			local function put(s)
				DrawString(x + 10, line, "LEFT", 14, "VAR", s)
				line = line + 16
			end
			put("u46 " .. mode .. " n=" .. #self.compareEntries .. " active=" .. self.activeCompareIndex
				.. " " .. entry.label .. " short=" .. self:GetShortBuildName(self.primaryBuild.buildName)
				.. " cfg=" .. #self.configControlList .. "/" .. #self.configSections
				.. " specs=" .. #self.primaryBuild.treeTab.specList)
			if mode == "CONFIG" then
				for _, sec in ipairs(self.configSections) do
					local s = sec.name .. " " .. #sec.items
					for i = 1, math.min(3, #sec.items) do
						local it = sec.items[i]
						s = s .. " " .. it.varData.var .. "=" .. u46val(it.varData, it.primaryControl) .. "/" .. u46val(it.varData, it.compareControl)
							.. ":" .. self:FormatConfigValue(it.varData, entry.configTab.input[it.varData.var])
					end
					put(s)
				end
			elseif mode == "ITEMS" then
				for _, j in ipairs(self:GetJewelComparisonSlots(entry)) do
					put("J " .. j.label .. " " .. j.nodeId .. " " .. (j.pItem and j.pItem.name or "-") .. "/" .. (j.cItem and j.cItem.name or "-")
						.. " " .. tostring(j.pNodeAllocated) .. " " .. tostring(j.cNodeAllocated) .. " " .. j.pSlotName)
				end
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
    /// The stand-in `spec.modFlag` (UndoHandler.lua:37 via `spec:AddUndoState`, U25).
    static SPEC_MOD_FLAG: Cell<bool> = const { Cell::new(false) };
    /// The PoB source dir (the entries' LuaCalcBridge).
    static SRC: RefCell<Option<PathBuf>> = const { RefCell::new(None) };
    /// Drop the window titles while set (the entry constructor's, see the module notes).
    static DROP_TITLES: Cell<bool> = const { Cell::new(false) };
}

/// `new("CompareEntry"):CompareEntry(xmlText, label)` stand-in (module notes): the entry's
/// model from `Init_model` and a LuaCalcBridge BuildOutput.
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
    cx.w::<CheckBoxControl>(id).label_width
}

/// The Lua stub's `u46val(varData, ctrl)`.
fn u46val(cx: &Cx, ty: Option<&str>, id: CtrlId) -> String {
    match ty {
        Some("check") => cx.w::<CheckBoxControl>(id).state.to_string(),
        Some("list") => cx.w::<DropDownControl>(id).sel_index.to_string(),
        _ => cx.w::<EditControl>(id).buf.clone(),
    }
}

/// The stand-in `CompareTab:Draw` (the Lua stub's `ct:Draw`, statement for statement).
fn draw_compare(cx: &mut Cx, vp: &ViewPort, ev: &mut InputEvents) {
    let me = ci::tab(cx).expect("compareTab");
    pob_app::main_mode::background::draw_background(cx, vp);
    let anchor = c(cx, me, "subTabAnchor");
    at(cx, anchor, vp.x + 4.0, vp.y + 96.0);
    if !ci::st(cx, me).compareEntries.is_empty() {
        cx.sg.set_draw_color(0.25, 0.25, 0.25);
        cx.sg.draw_image(None, vp.x + 4.0, vp.y + 32.0, vp.width - 8.0, 2.0);
        cx.sg.draw_image(None, vp.x + 4.0, vp.y + 88.0, vp.width - 8.0, 2.0);
        cx.sg.draw_image(None, vp.x + 4.0, vp.y + 122.0, vp.width - 8.0, 2.0);
    }
    let (x, y, w, h) = (vp.x, vp.y + 126.0, vp.width, vp.height - 126.0);
    let entry = CompareTab::GetActiveCompare(cx, me);
    let mode = ci::st(cx, me).compareViewMode;
    if let Some(entry) = &entry {
        CompareTab::PopulateSetDropdown(cx, SetTab::Skills(&entry.model.skillsTab), c(cx, me, "compareSkillSetSelect"));
        CompareTab::PopulateSetDropdown(cx, SetTab::Items(&entry.model.itemsTab), c(cx, me, "compareItemSetSelect"));
        CompareTab::PopulateSetDropdown(cx, SetTab::Config(&entry.model.configTab), c(cx, me, "compareConfigSetSelect"));
        match mode {
            "CONFIG" => {
                let s = ci::st(cx, me);
                if s.configCompareId != Some(s.activeCompareIndex) || s.configNeedsRebuild {
                    CompareTab::RebuildConfigControls(cx, me, Some(entry));
                    let s = ci::st_mut(cx, me);
                    s.configCompareId = Some(s.activeCompareIndex);
                    s.configNeedsRebuild = false;
                }
                at(cx, c(cx, me, "copyConfigBtn"), x + 315.0, y + 4.0);
                at(cx, c(cx, me, "configToggleBtn"), x + 315.0, y + 28.0);
                at(cx, c(cx, me, "configSearchEdit"), x + 10.0, y + 28.0);
                at(cx, c(cx, me, "configPrimarySetLabel"), x + 10.0, y + 6.0);
                at(cx, c(cx, me, "configPrimarySetSelect"), x + 80.0, y + 4.0);
            }
            "TREE" => {
                at(cx, c(cx, me, "leftSpecSelect"), x + 10.0, y + 30.0);
                at(cx, c(cx, me, "rightSpecSelect"), x + 420.0, y + 30.0);
                let t = c(cx, me, "treeOverlayCheck");
                at(cx, t, x + 10.0 + label_width(cx, t), y + 56.0);
                at(cx, c(cx, me, "overlayTreeSearch"), x + 200.0, y + 56.0);
                at(cx, c(cx, me, "leftFooterAnchor"), x + 10.0, y + 80.0);
                at(cx, c(cx, me, "rightFooterAnchor"), x + 420.0, y + 80.0);
            }
            "ITEMS" => {
                let e = c(cx, me, "itemsExpandedCheck");
                at(cx, e, x + 10.0 + label_width(cx, e), y + 8.0);
                at(cx, c(cx, me, "primaryItemSetLabel"), x + 10.0, y + 54.0);
                at(cx, c(cx, me, "primaryItemSetSelect"), x + 80.0, y + 52.0);
                at(cx, c(cx, me, "compareItemSetLabel2"), x + 420.0, y + 54.0);
                at(cx, c(cx, me, "compareItemSetSelect2"), x + 490.0, y + 52.0);
                at(cx, c(cx, me, "primaryTreeSetLabel"), x + 10.0, y + 80.0);
                at(cx, c(cx, me, "primaryTreeSetSelect"), x + 80.0, y + 78.0);
                at(cx, c(cx, me, "compareTreeSetLabel"), x + 420.0, y + 80.0);
                at(cx, c(cx, me, "compareTreeSetSelect"), x + 490.0, y + 78.0);
                at(cx, c(cx, me, "u46CopyItem"), x + 200.0, y + 8.0);
            }
            "SUMMARY" => {
                at(cx, c(cx, me, "comparePowerStatSelect"), x + 10.0, y + 60.0);
                for (key, dx) in [
                    ("comparePowerTreeCheck", 250.0),
                    ("comparePowerItemsCheck", 330.0),
                    ("comparePowerGemsCheck", 410.0),
                    ("comparePowerSupportGemsCheck", 520.0),
                    ("comparePowerConfigCheck", 650.0),
                ] {
                    let id = c(cx, me, key);
                    at(cx, id, x + dx + label_width(cx, id), y + 61.0);
                }
                at(cx, c(cx, me, "comparePowerReportList"), x + 10.0, y + 90.0);
            }
            "CALCS" => {
                let id = c(cx, me, "calcsShowOnlyDifferencesCheck");
                at(cx, id, x + 10.0 + label_width(cx, id), y + 8.0);
            }
            _ => {}
        }
    }
    cx.process_controls_input(me, ev, vp);
    match &entry {
        None => {
            cx.sg.set_viewport(x, y, w, h);
            cx.sg.set_draw_color(1.0, 1.0, 1.0);
            cx.sg
                .draw_string(0.0, 40.0, Align::Center, 20.0, Font::Var, "^7No comparison build loaded.");
            let pos = cx.main.cc.POSITIVE.clone();
            cx.sg.draw_string(
                0.0,
                70.0,
                Align::Center,
                16.0,
                Font::Var,
                &format!("^7Click {pos}Import...^7 above to import a build to compare against."),
            );
            cx.sg.reset_viewport();
        }
        Some(entry) => {
            cx.sg.set_draw_color(1.0, 1.0, 1.0);
            let mut lines: Vec<String> = Vec::new();
            let s = ci::st(cx, me);
            let p = cx.build().model.clone();
            lines.push(format!(
                "u46 {mode} n={} active={} {} short={} cfg={}/{} specs={}",
                s.compareEntries.len(),
                s.activeCompareIndex,
                entry.label,
                CompareTab::GetShortBuildName(cx.build().build_name.as_deref()),
                s.configControlList.len(),
                s.configSections.len(),
                p.treeTab.specList.len()
            ));
            if mode == "CONFIG" {
                for sec in &s.configSections {
                    let items = sec.items.borrow();
                    let mut l = format!("{} {}", sec.name, items.len());
                    for it in items.iter().take(3) {
                        let ty = it.varData.r#type.as_deref();
                        let var = it.varData.var.expect("var");
                        let v = entry.model.configTab.input.get(var);
                        l.push_str(&format!(
                            " {var}={}/{}:{}",
                            u46val(cx, ty, it.primaryControl),
                            u46val(cx, ty, it.compareControl),
                            CompareTab::FormatConfigValue(cx, it.varData, &v)
                        ));
                    }
                    lines.push(l);
                }
            } else if mode == "ITEMS" {
                for j in CompareTab::GetJewelComparisonSlots(cx, entry) {
                    let name = |i: &Option<pob_app::model::ItemRef>| i.as_ref().map_or("-".to_owned(), |i| i.name.get().to_string());
                    lines.push(format!(
                        "J {} {} {}/{} {} {} {}",
                        j.label,
                        j.nodeId,
                        name(&j.pItem),
                        name(&j.cItem),
                        j.pNodeAllocated,
                        j.cNodeAllocated,
                        j.pSlotName
                    ));
                }
            }
            let mut line = y + 110.0;
            for l in lines {
                cx.sg.draw_string(x + 10.0, line, Align::Left, 14.0, Font::Var, &l);
                line += 16.0;
            }
        }
    }
    cx.draw_controls(me, vp, None);
}

fn rust_stubs() {
    SPEC_MOD_FLAG.with(|f| f.set(false));
    init::set_hooks(|h| {
        *h = init::default_hooks();
        h.spec_add_undo_state = Rc::new(|_| SPEC_MOD_FLAG.with(|f| f.set(true)));
        // the stub's test button, added after the constructor (as the Lua stub does)
        let ctor = h.compare_tab.clone();
        h.compare_tab = Rc::new(move |cx, build| {
            let me = ctor(cx, build);
            let b = cx
                .add(
                    me,
                    "u46CopyItem",
                    ButtonControl::new(Anchor::NONE, rect(0, 0, 120, 20), "u46 copy ring", move |cx| {
                        if let Some(entry) = CompareTab::GetActiveCompare(cx, me) {
                            CompareTab::CopyCompareItemToPrimary(cx, "Ring 1", &entry, true);
                        }
                    }),
                )
                .id();
            cx.base_mut(b).shown = Prop::f(move |cx, _| {
                ci::st(cx, me).compareViewMode == "ITEMS" && CompareTab::GetActiveCompare(cx, me).is_some()
            });
            me
        });
    });
    ci::set_compare_hooks(|h| {
        *h = ci::default_compare_hooks();
        h.new_compare_entry = Rc::new(compare_entry);
        // the power report list draws nothing (as the Lua stub's); U48's own scenarios are u48
        h.power_report_list = Rc::new(|cx, me| {
            cx.add(
                me,
                "comparePowerReportList",
                New::new(Base::new(Anchor::NONE, rect(0, 0, 750, 250)), ci::PendingPowerReportList),
            )
            .id()
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
        let (flag, set_flag) = (h.tab_flag.clone(), h.set_tab_flag.clone());
        h.tab_flag = Rc::new(move |cx, f| {
            if f == TabFlag::Spec {
                SPEC_MOD_FLAG.with(Cell::get)
            } else {
                flag(cx, f)
            }
        });
        h.set_tab_flag = Rc::new(move |cx, f, v| {
            if f == TabFlag::Spec {
                SPEC_MOD_FLAG.with(|c| c.set(v));
            } else {
                set_flag(cx, f, v);
            }
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

/// Records the platform side effects the Lua side's bridge records (as in u16-u45).
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
            Rc::new(move || Box::new(LuaCalcBridge::new(Some(src.clone())))),
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

// ---- T4 (the rules are u20's; see its module notes) ----

fn unordered_children(elem: &str) -> bool {
    matches!(elem, "Calcs" | "ConfigSet" | "ItemSet" | "Sockets" | "Overrides")
}

/// The UI-only savers compared separately (a difference is reported, not failed).
const PENDING_SAVERS: [&str; 2] = ["Import", "TreeView"];

#[derive(Default)]
struct T4Notes {
    pending_sections: Vec<String>,
    gem_nil_bools: usize,
    ambiguous_variants: usize,
}

fn ambiguous_variant(node: &XmlNode) -> bool {
    let (Some(game_id), Some(skill_id)) = (node.attrib.get("gemId"), node.attrib.get("skillId")) else {
        return false;
    };
    let n = pob_gamedata::data()
        .gems
        .values()
        .filter(|g| *g.gameId == **game_id && *g.grantedEffectId == **skill_id)
        .count();
    n > 1
}

fn normalise(node: &XmlNode, top: bool, lua: bool, notes: &mut T4Notes) -> String {
    let mut attrs: Vec<(String, String)> = Vec::new();
    for (k, v) in &node.attrib {
        let rust_default = match (&*node.elem, &**k) {
            ("Gem", "enableGlobal1" | "enableGlobal2") | ("Skill", "includeInFullDPS") => Some("false"),
            ("Skill", "mainActiveSkill" | "mainActiveSkillCalcs") => Some("0"),
            _ => None,
        };
        if let Some(d) = rust_default.filter(|_| lua && &**v == "nil") {
            notes.gem_nil_bools += 1;
            attrs.push((k.to_string(), d.to_owned()));
        } else if &*node.elem == "Gem" && matches!(&**k, "variantId" | "nameSpec") && ambiguous_variant(node) {
            if k.as_ref() == "variantId" {
                notes.ambiguous_variants += 1;
            }
            attrs.push((k.to_string(), "<pairs-chosen variant>".to_owned()));
        } else {
            attrs.push((k.to_string(), v.to_string()));
        }
    }
    attrs.sort();
    let mut kids: Vec<String> = Vec::new();
    for ch in &node.children {
        match ch {
            XmlChild::Node(n) => {
                if top && notes.pending_sections.iter().any(|p| **p == *n.elem) {
                    continue;
                }
                kids.push(normalise(n, false, lua, notes));
            }
            XmlChild::Text(t) => kids.push(format!("{t:?}")),
        }
    }
    if top || unordered_children(&node.elem) {
        kids.sort();
    }
    format!("<{} {:?}>[{}]", node.elem, attrs, kids.join(","))
}

fn root_element(text: &str) -> Result<XmlNode, String> {
    let root = pob_calc::model::xml::ParseXML_root(text)?;
    root.children
        .into_iter()
        .find_map(|ch| match ch {
            XmlChild::Node(n) => Some(n),
            XmlChild::Text(_) => None,
        })
        .ok_or_else(|| "no root element".to_owned())
}

fn first_diff(a: &str, b: &str) -> String {
    let (a, b): (Vec<&str>, Vec<&str>) = (a.split('<').collect(), b.split('<').collect());
    for (i, (x, y)) in a.iter().zip(b.iter()).enumerate() {
        if x != y {
            return format!("element {i}: lua <{x} | rust <{y}");
        }
    }
    format!("lengths {} vs {}", a.len(), b.len())
}

/// Compare one T4 pair; `Ok(byte_identical)`.
fn compare_t4(what: &str, lua: &str, rust: &str, notes: &mut T4Notes) -> Result<bool, String> {
    let lr = root_element(lua).map_err(|e| format!("{what}: Lua XML: {e}"))?;
    let rr = root_element(rust).map_err(|e| format!("{what}: Rust XML: {e}"))?;
    let mut local = T4Notes::default();
    let section = |root: &XmlNode, elem: &str, lua: bool| {
        root.children.iter().find_map(|ch| match ch {
            XmlChild::Node(n) if *n.elem == *elem => Some(normalise(n, false, lua, &mut T4Notes::default())),
            _ => None,
        })
    };
    for elem in PENDING_SAVERS {
        if section(&lr, elem, true) != section(&rr, elem, false) {
            local.pending_sections.push(elem.to_owned());
        }
    }
    let ln = normalise(&lr, true, true, &mut local);
    let rn = normalise(&rr, true, false, &mut local);
    notes.gem_nil_bools += local.gem_nil_bools;
    notes.ambiguous_variants += local.ambiguous_variants;
    notes.pending_sections.extend(local.pending_sections);
    if ln != rn {
        return Err(format!("{what}: T4 differs: {}", first_diff(&ln, &rn)));
    }
    Ok(lua == rust)
}

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
        "ui-u46",
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
    if std::env::var_os("U46_DUMP_POS").is_some() {
        dump_positions(&rust_side.app.cx);
    }
    drop(rust_side);
    drop_image_loads(&mut frames);
    let rust = Side {
        frames,
        error,
        snapshots: std::mem::take(&mut d.snapshots),
    };
    Ok((lua, rust))
}

/// Print where the build's mode buttons and the Compare tab's controls are (U46_DUMP_POS=1).
fn dump_positions(cx: &Cx) {
    let Some(bs) = cx.app().build.as_ref() else {
        println!("U46_DUMP_POS: no build");
        return;
    };
    for key in ["modeParty", "modeCompare"] {
        if let Some(id) = cx.child(bs.node.id(), key) {
            let (x, y) = cx.pos(id);
            println!("U46_DUMP_POS {key}: ({x}, {y})");
        }
    }
    let me = bs.tabs.compare.id();
    for key in DUMP_KEYS {
        if let Some(id) = cx.child(me, *key) {
            let (x, y) = cx.pos(id);
            let (w, h) = cx.size(id);
            if cx.is_shown(id) {
                println!("U46_DUMP_POS {key}: ({x}, {y}) {w}x{h}");
            }
        }
    }
    for p in &cx.main.popups {
        for key in POPUP_KEYS {
            if let Some(id) = cx.child(*p, *key) {
                let (x, y) = cx.pos(id);
                let (w, h) = cx.size(id);
                println!("U46_DUMP_POS popup {key}: ({x}, {y}) {w}x{h}");
            }
        }
    }
}

const DUMP_KEYS: &[&str] = &[
    "subTabSummary", "subTabTree", "subTabSkills", "subTabItems", "subTabCalcs", "subTabConfig",
    "compareBuildLabel", "compareBuildSelect", "importBtn", "reimportBtn", "removeBtn",
    "compareSpecSelect", "compareSkillSetSelect", "compareItemSetSelect", "compareConfigSetSelect",
    "cmpSocketGroup", "copyConfigBtn", "configToggleBtn", "configSearchEdit", "configPrimarySetSelect",
    "leftSpecSelect", "leftVersionSelect", "rightSpecSelect", "rightVersionSelect", "copySpecBtn",
    "copySpecUseBtn", "treeOverlayCheck", "overlayTreeSearch", "leftTreeSearch", "rightTreeSearch",
    "itemsExpandedCheck", "primaryItemSetSelect", "compareItemSetSelect2", "primaryTreeSetSelect",
    "compareTreeSetSelect", "u46CopyItem", "comparePowerStatSelect", "comparePowerTreeCheck",
    "comparePowerConfigCheck", "calcsShowOnlyDifferencesCheck",
];

const POPUP_KEYS: &[&str] = &[
    "label", "input", "name", "state", "go", "importFolder", "cancel", "searchText", "sort",
    "buildList", "open", "close",
];

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

/// Every T4 pair of a run; `Ok(number of pairs, byte-identical pairs)`.
fn t4(lua: &Side, rust: &Side, notes: &mut T4Notes) -> Result<(usize, usize), Vec<String>> {
    let mut errs = Vec::new();
    let (mut n, mut same) = (0, 0);
    if lua.snapshots.len() != rust.snapshots.len() {
        errs.push(format!("snapshots: lua {} rust {}", lua.snapshots.len(), rust.snapshots.len()));
    }
    for ((ln, lx), (_, rx)) in lua.snapshots.iter().zip(&rust.snapshots) {
        n += 1;
        match compare_t4(&format!("snapshot {ln}"), lx, rx, notes) {
            Ok(b) => same += usize::from(b),
            Err(e) => errs.push(e),
        }
    }
    if errs.is_empty() {
        Ok((n, same))
    } else {
        Err(errs)
    }
}

/// `#! u46-expect-text TEXT`: some Lua DrawString contains TEXT (guards a script whose clicks
/// silently stopped reaching the tab, which both sides would agree on); `#! u46-expect-snapshot
/// NAME TEXT`: the Lua snapshot NAME contains TEXT.
fn check_expectations(text: &str, lua: &Side) -> Vec<String> {
    let mut errs = Vec::new();
    let texts = draw_texts(&lua.frames);
    for want in directives(text, "u46-expect-text") {
        if !texts.iter().any(|t| t.contains(want.as_str())) {
            errs.push(format!("no Lua DrawString contains {want:?}"));
        }
    }
    for d in directives(text, "u46-expect-snapshot") {
        let (name, want) = d.split_once(' ').unwrap_or((d.as_str(), ""));
        let found = lua.snapshots.iter().any(|(n, x)| n == name && x.contains(want));
        if !found {
            errs.push(format!("Lua snapshot {name} does not contain {want:?}"));
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
    t4_pairs: usize,
    t4_bytes: usize,
}

#[test]
fn u46_scenarios() {
    let Some(root) = inputs() else { return };
    let mut scripts: Vec<PathBuf> = std::fs::read_dir(test_dir())
        .expect("tests/u46")
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|x| x == "pobscript"))
        .collect();
    scripts.sort();
    assert!(!scripts.is_empty(), "no tests/u46/*.pobscript");
    let filter = std::env::var("U46_FILTER").ok();
    let only_screen = std::env::var("U46_SCREEN").ok();
    let mut cases = Vec::new();
    for path in scripts {
        let stem = path.file_stem().unwrap().to_string_lossy().into_owned();
        if filter.as_ref().is_some_and(|f| !stem.contains(f.as_str())) {
            continue;
        }
        let (script, _) = runner::load_script(&path).unwrap();
        let own_screen = matches!(script.steps.first(), Some(pob_host::script::Step::Screen { .. }));
        let screens: Vec<(i32, i32, f32)> = if own_screen {
            vec![(1280, 800, 1.0)]
        } else {
            SCREENS.to_vec()
        };
        for (w, h, s) in screens {
            if only_screen.as_ref().is_some_and(|o| *o != w.to_string()) {
                continue;
            }
            cases.push((path.clone(), stem.clone(), own_screen, (w, h, s)));
        }
    }
    let results = oracle_cache::par_map(cases, |(path, stem, own_screen, (w, h, s))| {
        let mut out = CaseOut::default();
        let text = std::fs::read_to_string(&path).unwrap();
        let (script, base_dir) = runner::load_script(&path).unwrap();
        let fx = runner::fixtures(&script, &base_dir).unwrap();
        {
            let tag = if own_screen {
                format!("u46_{stem}")
            } else {
                format!("u46_{stem}@{w}x{h}@{s}")
            };
            let (lua, rust) = match run_sides(&root, &script, fx.user_dir.as_deref(), runner::screen(w, h, s)) {
                Ok(s) => s,
                Err(e) => {
                    out.failures.push(format!("{tag}: {e:#}"));
                    return out;
                }
            };
            out.ran += 1;
            let titles = (window_titles(&lua.frames), window_titles(&rust.frames));
            let mut notes = T4Notes::default();
            let t4r = t4(&lua, &rust, &mut notes);
            let expect = check_expectations(&text, &lua);
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
            let t4_line = match &t4r {
                Ok((p, b)) => {
                    out.t4_pairs += p;
                    out.t4_bytes += b;
                    let mut pending = notes.pending_sections.clone();
                    pending.sort();
                    pending.dedup();
                    format!(
                        "T4 {p} identical ({b} byte-identical; set aside: {} Lua nil gem/group fields, {} pairs-chosen gem variants, pending UI-saver sections differing {:?})",
                        notes.gem_nil_bools, notes.ambiguous_variants, pending
                    )
                }
                Err(e) => format!("T4 FAILED: {}", e.join("; ")),
            };
            out.lines.push(format!(
                "{tag}: {} frames, {ops} Lua draw ops, {n} divergences, {t4_line}, titles {:?}{}{}{}",
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
                || t4r.is_err()
                || !expect.is_empty()
            {
                out.failures.push(format!(
                    "{tag}: {n} divergences, frames lua {} rust {}, {ops} ops, {t4_line}, titles lua {:?} rust {:?}, errors lua {:?} rust {:?}, expectations {expect:?} (see {})",
                    run.lua.len(),
                    run.rust.len(),
                    distinct_titles(&titles.0),
                    distinct_titles(&titles.1),
                    lua.error,
                    rust.error,
                    out_dir().join(&tag).join("report.txt").display()
                ));
            }
        }
        out
    });
    let mut failures = Vec::new();
    let mut ran = 0;
    let (mut t4_pairs, mut t4_bytes) = (0, 0);
    for case in results {
        for l in &case.lines {
            println!("{l}");
        }
        failures.extend(case.failures);
        ran += case.ran;
        t4_pairs += case.t4_pairs;
        t4_bytes += case.t4_bytes;
    }
    println!("u46: {ran} runs, T4 {t4_pairs} pairs ({t4_bytes} byte-identical)");
    println!("{}", oracle_cache::summary());
    assert!(ran > 0 || filter.is_some(), "no run");
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}
