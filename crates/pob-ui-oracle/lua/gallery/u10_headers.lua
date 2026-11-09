-- Gallery "u10_headers" (U10): tooltips drawn directly (no hover box) in pages, with
-- main.showFlavourText on and off: rarity headers (RELIC with foil colours, UNIQUE/RARE with
-- influence icons, MAGIC, NORMAL, GEM with its skipped first separator), passive headers
-- (NOTABLE with a recipe, KEYSTONE, ASCENDANCY, MASTERY, MASTERYALLOC, JEWEL, PASSIVE, an unknown
-- one), line backgrounds with the GetDrawColor save/restore across wrapped lines, multi-column
-- overflow (static and as a hover tooltip that shifts left), and child tooltips.
-- Rust counterpart: src/gallery/u10_headers.rs. Interactions: u10_headers.pobscript.

local G = { }

local function itemLines(tt, rarity, name)
	tt:AddLine(20, colorCodes[rarity] .. name, "FONTIN SC")
	tt:AddLine(20, colorCodes[rarity] .. "Leather Belt", "FONTIN SC")
	tt:AddSeparator(10)
	tt:AddLine(16, "^x7F7F7FRequires Level ^768", "FONTIN SC")
	tt:AddSeparator(10)
	tt:AddLine(16, colorCodes.MAGIC .. "+30 to maximum Life", "FONTIN SC")
	tt:AddLine(16, colorCodes.MAGIC .. "+12% to Fire Resistance", "FONTIN SC")
end

function G:Init()
	self.host = new("ControlHost"):ControlHost()
	local c = self.host.controls
	self.page = 1
	c.next = new("ButtonControl"):ButtonControl(nil, {10, 10, 80, 20}, "Next", function()
		self.page = self.page % 4 + 1
	end)
	c.flavour = new("ButtonControl"):ButtonControl({"LEFT", c.next, "RIGHT"}, {10, 0, 80, 20}, "Flavour", function()
		main.showFlavourText = not main.showFlavourText
	end)
	c.page = new("LabelControl"):LabelControl({"LEFT", c.flavour, "RIGHT"}, {10, 0, 0, 16}, function()
		return "^7page " .. self.page .. "  flavour " .. tostring(main.showFlavourText)
	end)
	c.overflow = new("ButtonControl"):ButtonControl(nil, {function() return main.screenW - 90 end, 10, 80, 20}, "Overflow", function() end)
	c.overflow.tooltipFunc = function(tooltip)
		if tooltip:CheckForUpdate(main.showFlavourText) then
			self:OverflowLines(tooltip, "Hover")
		end
	end
	self.tts = { }
	for i = 1, 8 do
		self.tts[i] = new("Tooltip"):Tooltip()
	end
end

function G:OverflowLines(tt, label)
	tt.center = true
	tt:AddLine(16, "^7" .. label .. " overflow tooltip")
	for b = 1, 9 do
		tt:AddLine(16, "^7Equipping block " .. b .. " will give you:")
		for l = 1, 6 do
			tt:AddLine(14, "^x88AAFF" .. "line " .. l .. " of block " .. b .. (l == 3 and " (a longer line here)" or ""))
		end
		tt:AddSeparator(8)
		tt:AddLine(14, "^7Removing it will give you:")
		tt:AddLine(14, "^1-10 to something")
	end
end

function G:Build(i, tt)
	local p = self.page
	tt.influenceHeader1 = nil
	tt.influenceHeader2 = nil
	tt.childTooltips = nil
	if p == 1 then
		if i == 1 then
			tt.tooltipHeader = "UNIQUE"
			tt.influenceHeader1 = "Shaper"
			tt.influenceHeader2 = "Elder"
			tt.center = true
			tt.color = colorCodes.UNIQUE
			tt.maxWidth = 360
			itemLines(tt, "UNIQUE", "Headhunter")
			tt:AddSeparator(10)
			tt:AddLine(16, colorCodes.MEMORY .. "Memory Strands: ^7 12 and some words so that it wraps onto more lines", "FONTIN SC", nil, "Assets/memorybg.png")
			tt:AddLine(16, colorCodes.INTANGIBILITY .. "Intangibility: ^7 5% and also enough words to wrap a few times", "FONTIN SC", nil, "Assets/intangibilitybg.png")
			tt:AddLine(16, "after the backgrounds", "FONTIN SC")
		elseif i == 2 then
			tt.tooltipHeader = "RARE"
			tt.influenceHeader1 = "Hunter"
			tt.influenceHeader2 = "Hunter"
			tt.center = true
			tt.color = colorCodes.RARE
			itemLines(tt, "RARE", "Doom Cord")
		elseif i == 3 then
			tt.tooltipHeader = "rare"
			tt.color = { 0.2, 0.4, 0.9 }
			itemLines(tt, "RARE", "Lower-case header")
		end
	elseif p == 2 then
		if i == 1 then
			tt.tooltipHeader = "MAGIC"
			tt.center = true
			tt.color = colorCodes.MAGIC
			itemLines(tt, "MAGIC", "Sapphire Ring of the Whale")
		elseif i == 2 then
			tt.tooltipHeader = "NORMAL"
			tt.influenceHeader1 = "Crusader"
			tt.influenceHeader2 = "Warlord"
			tt.color = colorCodes.NORMAL
			itemLines(tt, "NORMAL", "Iron Ring")
		elseif i == 3 then
			tt.tooltipHeader = "GEM"
			tt.center = true
			tt.color = colorCodes.GEM
			tt:AddLine(20, colorCodes.GEM .. "Fireball", "FONTIN SC")
			tt:AddSeparator(10)
			tt:AddLine(16, "^x7F7F7FLevel: ^720", "FONTIN SC")
			tt:AddSeparator(10)
			tt:AddLine(16, colorCodes.GEM .. "Unleashes a ball of fire.", "FONTIN SC", nil, "Assets/memorybg.png")
		elseif i == 4 then
			tt.tooltipHeader = "RELIC"
			tt.foilType = "Verdant"
			tt.center = true
			itemLines(tt, "RELIC", "Relic belt")
		elseif i == 5 then
			tt.tooltipHeader = "RELIC"
			tt.foilType = "Not a foil"
			tt.influenceHeader1 = "Synthesis"
			tt.influenceHeader2 = "Synthesis"
			itemLines(tt, "RELIC", "Unknown foil")
		end
	elseif p == 3 then
		local headers = { "NOTABLE", "KEYSTONE", "ASCENDANCY", "MASTERY", "MASTERYALLOC", "JEWEL", "PASSIVE", "UNKNOWN" }
		tt.tooltipHeader = headers[i]
		tt.center = i % 2 == 1
		if i == 1 then
			tt:SetRecipe({ "GoldenOil", "SilverOil", "OpalescentOil" })
		elseif i == 2 then
			tt:SetRecipe({ "AmberOil", "CrimsonOil", "TealOil", "VioletOil" })
		end
		tt:AddLine(18, "^7" .. headers[i]:lower() .. " node", "FONTIN SC")
		tt:AddSeparator(10)
		tt:AddLine(16, "^x8888FF10% increased Damage")
		tt:AddLine(16, "^x8888FF+5 to Strength")
	elseif p == 4 then
		if i == 1 then
			self:OverflowLines(tt, "Static")
		elseif i == 2 then
			tt.tooltipHeader = "UNIQUE"
			tt.center = true
			itemLines(tt, "UNIQUE", "Parent with children")
			tt.childTooltips = { new("Tooltip"):Tooltip(), new("Tooltip"):Tooltip() }
			for k, child in ipairs(tt.childTooltips) do
				child.tooltipHeader = "GEM"
				child.center = true
				child.color = colorCodes.GEM
				child:AddLine(18, colorCodes.GEM .. "Child gem " .. k, "FONTIN SC")
				child:AddSeparator(10)
				for l = 1, 8 do
					child:AddLine(14, "^7child line " .. l)
				end
			end
		end
	end
end

-- where page p draws tooltip i
local positions = {
	{ { 20, 50 }, { 420, 50 }, { 820, 50 } },
	{ { 20, 50 }, { 350, 50 }, { 700, 50 }, { 20, 400 }, { 400, 400 } },
	{ { 20, 50 }, { 320, 50 }, { 620, 50 }, { 920, 50 }, { 20, 330 }, { 320, 330 }, { 620, 330 }, { 920, 330 } },
	{ { 0, 40 }, { 900, 300 } },
}

function G:OnFrame(inputEvents, viewPort)
	self.host:ProcessControlsInput(inputEvents, viewPort)
	SetDrawColor(0.1, 0.1, 0.1)
	DrawImage(nil, viewPort.x, viewPort.y, viewPort.width, viewPort.height)
	for i, pos in ipairs(positions[self.page]) do
		local tt = self.tts[i]
		if tt:CheckForUpdate(self.page, main.showFlavourText) then
			self:Build(i, tt)
		end
		tt:Draw(pos[1], pos[2], nil, nil, viewPort)
	end
	self.host:DrawControls(viewPort)
end

return G
