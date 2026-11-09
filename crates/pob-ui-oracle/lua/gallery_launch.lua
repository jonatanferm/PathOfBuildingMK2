-- pob-ui-oracle: the gallery mini-launch (ui doc §11.2 "gallery harness"). It stands in for
-- Launch.lua when a widget gallery runs: it loads Modules/Common.lua, Data/Global.lua and
-- Modules/Main.lua (methods only: Main.lua's heavy data modules are skipped and main:Init is not
-- run), then runs lua/gallery/<name>.lua (path in `__galleryPath`).
--
-- A gallery file returns a table G with an optional `G:Init()` and a required
-- `G:OnFrame(inputEvents, viewPort)`; typically it builds a ControlHost from PoB classes and
-- calls `host:ProcessControlsInput(inputEvents, viewPort)` and `host:DrawControls(viewPort)`.
-- Its Rust counterpart is src/gallery/<name>.rs (see src/gallery.rs). Errors are not caught:
-- they fail the run.

local t_insert = table.insert

launch = {
	devMode = false,
	installedMode = false,
	versionNumber = "?",
	subScripts = { },
}
function launch:ShowErrMsg(fmt, ...)
	error(string.format(fmt, ...))
end
function launch:DownloadPage()
	error("launch:DownloadPage is not available in a gallery")
end
SetMainObject(launch)

LoadModule("Modules/Common")
LoadModule("Data/Global")

-- Main.lua, methods only: its own module loads are skipped except the light ones.
local skip = {
	["Modules/Common"] = true,
	["Modules/Data"] = true,
	["Modules/ModTools"] = true,
	["Modules/PantheonTools"] = true,
	["Modules/BuildSiteTools"] = true,
	["Modules.ItemTools"] = true,
	["Modules.CalcTools"] = true,
}
local raw_LoadModule, raw_require = LoadModule, require
LoadModule = function(name, ...)
	if skip[name] then
		return
	end
	return raw_LoadModule(name, ...)
end
require = function(name)
	if skip[name] then
		return
	end
	return raw_require(name)
end
LoadModule("Modules/Main")
LoadModule, require = raw_LoadModule, raw_require

-- The main:Init fields that generic controls read (Main.lua:59-125, defaults; no Settings.xml).
main:DetectUnicodeSupport()
main.modes = { }
main.popups = { }
main.inputEvents = { }
main.tooltipLines = { }
main.onFrameFuncs = { }
main.buildSortMode = "NAME"
main.nodePowerTheme = "RED/BLUE"
main.colorPositive = defaultColorCodes.POSITIVE
main.colorNegative = defaultColorCodes.NEGATIVE
main.colorHighlight = defaultColorCodes.HIGHLIGHT
main.showThousandsSeparators = true
main.useCompactValues = false
main.edgeSearchHighlight = true
main.thousandsSeparator = ","
main.decimalSeparator = "."
main.defaultItemAffixQuality = 0.5
main.showTitlebarName = true
main.dpiScaleOverridePercent = GetDPIScaleOverridePercent and GetDPIScaleOverridePercent() or 0
main.showWarnings = true
main.slotOnlyTooltips = true
main.migrateEldritchImplicits = true
main.notSupportedModTooltips = true
main.notSupportedTooltipText = " ^8(Not supported in PoB yet)"
main.showPublicBuilds = true
main.showFlavourText = true
main.showAnimations = true
main.showAllItemAffixes = true
main.disableScrollControlInteraction = false

local gallery

function launch:OnInit()
	gallery = LoadModule(__galleryPath)
	assert(type(gallery) == "table" and gallery.OnFrame, "gallery file must return a table with OnFrame")
	if gallery.Init then
		gallery:Init()
	end
end

function launch:OnFrame()
	main.screenW, main.screenH = GetVirtualScreenSize()
	main.viewPort = { x = 0, y = 0, width = main.screenW, height = main.screenH }
	gallery:OnFrame(main.inputEvents, main.viewPort)
	wipeTable(main.inputEvents)
end

function launch:OnKeyDown(key, doubleClick)
	t_insert(main.inputEvents, { type = "KeyDown", key = key, doubleClick = doubleClick })
end

function launch:OnKeyUp(key)
	t_insert(main.inputEvents, { type = "KeyUp", key = key })
end

function launch:OnChar(key)
	t_insert(main.inputEvents, { type = "Char", key = key })
end
