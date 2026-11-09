-- Gallery "u13_search" (U13): DropDownControl type-to-search (SearchHost) over table entries with
-- `detail` and `searchFilter` (ImportTab.lua:1071-1079 style) and over strings with
-- ignoreSearchOrder: the "Search: " box label coloured by the match count, the filtered rows
-- with their highlight rectangles (merged ranges), "<No matches>", BACK, ESCAPE (resets the
-- search, then closes), selecting a filtered row (DropIndexToListIndex); CheckDroppedWidth with
-- maxDroppedWidth (and the scroll bar width while dropped) and enableChangeBoxWidth.
-- Rust counterpart: src/gallery/u13_search.rs. Interactions: u13_search.pobscript.

local t_insert = table.insert

local G = { }

function G:Init()
	self.host = new("ControlHost"):ControlHost()
	local c = self.host.controls
	local status = "-"
	local names = { "Aria", "Bran", "Cael", "Dara", "Eryn", "Fen", "Gale", "Hale", "Iris", "Joss", "Kael", "Lune" }
	local classes = { "Witch", "Ranger", "Marauder", "Duelist", "Templar", "Shadow" }
	local chars = { }
	for i, name in ipairs(names) do
		local class = classes[(i - 1) % 6 + 1]
		t_insert(chars, { label = "^xAF6025" .. name, detail = "^x808080" .. class .. " lvl " .. (i * 7), searchFilter = name .. " " .. class, id = i })
	end
	c.chars = new("DropDownControl"):DropDownControl(nil, {20, 40, 260, 20}, chars, function(index, value)
		status = "chars " .. index .. " " .. value.id
	end)
	c.chars.tooltipFunc = function(tooltip, mode, index, value)
		tooltip:Clear()
		tooltip:AddLine(14, "^7" .. mode .. " " .. tostring(index) .. " " .. (value and value.searchFilter or "nil"))
	end
	local words = { "Fire", "Ice", "Lightning", "Chaos", "Physical" }
	local loose = { }
	for i = 1, 40 do
		t_insert(loose, words[(i - 1) % 5 + 1] .. " and " .. words[(i * 3) % 5 + 1] .. " damage, a rather long entry number " .. i)
	end
	c.loose = new("DropDownControl"):DropDownControl(nil, {320, 40, 200, 20}, loose, function(index, value)
		status = "loose " .. index
		c.loose:CheckDroppedWidth(true)
	end, nil, true)
	c.loose.maxDroppedWidth = 420
	c.loose:CheckDroppedWidth(true)
	local boxes = { "Short", "A somewhat longer entry", "An entry so long that the box would need far more than the cap of three hundred and ninety pixels", "Mid length entry" }
	c.box = new("DropDownControl"):DropDownControl(nil, {560, 40, 190, 20}, boxes, function(index, value)
		status = "box " .. index
		c.box:CheckDroppedWidth(true)
	end)
	c.box.enableChangeBoxWidth = true
	c.box:CheckDroppedWidth(true)
	c.status = new("LabelControl"):LabelControl(nil, {20, 10, 0, 16}, function()
		return "^7" .. status
	end)
end

function G:OnFrame(inputEvents, viewPort)
	self.host:ProcessControlsInput(inputEvents, viewPort)
	SetDrawColor(0.1, 0.1, 0.1)
	DrawImage(nil, viewPort.x, viewPort.y, viewPort.width, viewPort.height)
	self.host:DrawControls(viewPort)
end

return G
