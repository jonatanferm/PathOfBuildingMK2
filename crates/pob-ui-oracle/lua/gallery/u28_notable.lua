-- Gallery "u28_notable" (U28): the real NotableDBControl over a stand-in `itemsTab` (build with
-- outputRevision and calcsTab:GetMiscCalculator, displayItem, anointItem, socketViewer:AddNodeName,
-- AppendAnointTooltip) and a small node db (anointable or not, an empty modKey, an infinite power,
-- supported and unsupported description lines, reminder text). `data.powerStatList` is the real
-- one: Modules/Data.lua:129-235 run here (the mini-launch skips Modules/Data). Name sort (the
-- default), stat sorts through the ListBuilder coroutine (it never yields: the oracle clock does
-- not move within a frame), the "^x%X8080" power colours, search by name and by modifier with
-- matchOrPattern alternatives, the search mode dropdown, an outputRevision change rebuilding the
-- list, hover tooltips and a drag of a node. Rust counterpart: src/gallery/u28_notable.rs.
-- Interactions: u28_notable.pobscript.

data = data or { }
do
	local f = assert(io.open("Modules/Data.lua", "r"))
	local text = f:read("*a")
	f:close()
	local lines, i = { }, 0
	for line in (text .. "\n"):gmatch("([^\n]*)\n") do
		i = i + 1
		if i >= 129 and i <= 235 then
			lines[#lines + 1] = line
		end
	end
	assert(loadstring("local t_insert = table.insert\n" .. table.concat(lines, "\n")))()
end

local G = { }

function G:Init()
	-- Main.lua:120 (main:Init does not run in a gallery)
	main.notSupportedTooltipText = " ^8(Not supported in PoB yet)"
	local function calcFunc(override)
		local power = override.repItem.power
		return { FullDPS = power or 100, Life = (power or 100) / 2 }
	end
	local itemsTab = {
		build = {
			outputRevision = 1,
			calcsTab = {
				GetMiscCalculator = function(calcsTab)
					return calcFunc, { }
				end,
			},
		},
		displayItem = { base = { type = "Amulet" } },
		anointItem = function(itemsTab, node)
			return { power = node and node.fakePower }
		end,
		socketViewer = {
			AddNodeName = function(viewer, tooltip, node, build)
				tooltip:AddLine(20, "^7" .. node.dn)
			end,
		},
		AppendAnointTooltip = function(itemsTab, tooltip, node)
			tooltip:AddLine(14, "^7Anoint: " .. node.dn)
		end,
	}
	self.itemsTab = itemsTab
	local specs = {
		{ "Bravery", { "10% increased Damage", "Unsupported effect" }, { { list = { } }, { list = { }, extra = "x" } }, true, "k1", 50, { "Reminder one", "Reminder two" } },
		{ "Arsonist", { "Fire Damage" }, { { list = { } } }, true, "k2", 200 },
		{ "Not Anointable", { "Nothing" }, { { list = { } } }, false, "k3", 999 },
		{ "Cold Heart", { " ", "Cold Damage" }, { { }, { list = { } } }, true, "", 0 },
		{ "Divine Fury", { "Holy Fire" }, { { } }, true, "k5", 1 / 0 },
		{ "The Explosive", { "Area Damage" }, { { list = { } } }, true, "k6", 50 },
		{ "Arcane Focus", { "Spell Damage", "Cast Speed" }, { { list = { } }, { list = { } } }, true, "k7", 10 },
		{ "Heart of Ice", { "Cold Penetration" }, { { list = { } } }, true, "k8", 120 },
	}
	local db = { }
	for id, s in ipairs(specs) do
		db[id] = { id = id, dn = s[1], sd = s[2], mods = s[3], recipe = s[4] and { "AmberOil", "AzureOil" } or nil, modKey = s[5], fakePower = s[6], reminderText = s[7] }
	end
	self.host = new("ControlHost"):ControlHost()
	local c = self.host.controls
	c.db = new("NotableDBControl"):NotableDBControl(nil, {10, 100, 360, 300}, itemsTab, db, "ANOINT")
	c.rev = new("ButtonControl"):ButtonControl(nil, {420, 100, 160, 20}, "Output revision", function()
		itemsTab.build.outputRevision = itemsTab.build.outputRevision + 1
	end)
	c.target = new("DropDownControl"):DropDownControl(nil, {420, 140, 160, 20}, { "Drop target" })
	c.db.dragTargetList = { c.target }
	c.status = new("LabelControl"):LabelControl(nil, {20, 10, 0, 16}, function()
		return "^7sort " .. tostring(c.db.sortMode) .. " max " .. tostring(c.db.sortMaxPower) .. " rev " .. itemsTab.build.outputRevision .. " sel " .. tostring(c.db.selIndex)
	end)
end

function G:OnFrame(inputEvents, viewPort)
	self.host:ProcessControlsInput(inputEvents, viewPort)
	SetDrawColor(0.1, 0.1, 0.1)
	DrawImage(nil, viewPort.x, viewPort.y, viewPort.width, viewPort.height)
	self.host:DrawControls(viewPort)
	-- Modules/Main.lua:445-454 (main:OnFrame does not run in a gallery)
	if main.showDragText then
		local cursorX, cursorY = GetCursorPos()
		local strWidth = DrawStringWidth(16, "VAR", main.showDragText)
		SetDrawLayer(20, 0)
		SetDrawColor(0.15, 0.15, 0.15, 0.75)
		DrawImage(nil, cursorX, cursorY - 8, strWidth + 2, 18)
		SetDrawColor(1, 1, 1)
		DrawString(cursorX + 1, cursorY - 7, "LEFT", 16, "VAR", main.showDragText)
		main.showDragText = nil
	end
end

return G
