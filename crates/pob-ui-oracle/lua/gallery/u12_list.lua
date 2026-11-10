-- Gallery "u12_list" (U12): ListControl through a subclass (class hooks) and a plain
-- `new("ListControl")` with instance methods (ConfigModBrowser.lua style). Column labels with a
-- function width and a nil width, sortable headers (ReSort), the "..." ellipsis through
-- DrawStringCursorIndex, the list label, hover tooltips (AddValueTooltip), selection by click,
-- double click, UP/DOWN/HOME/END with wrap, Ctrl+C, DELETE, other keys (OnSelKeyDown), the wheel,
-- row separators, row icons (GetRowIcon), OverrideSelectIndex + SetHighlightColor (shift-select
-- highlight, TimelessJewelListControl style), a non-scrolling list and the defaultText of an empty
-- list. Rust counterpart: src/gallery/u12_list.rs. Interactions: u12_list.pobscript.

local t_insert = table.insert
local t_remove = table.remove

local TestList = newClass("U12TestList", "ListControl")

function TestList:U12TestList(anchor, rect, list)
	self:ListControl(anchor, rect, 16, "VERTICAL", false, list)
	self.colList = {
		{ width = 110, label = "Name", sortable = true },
		{ width = function() return self:GetProperty("width") - 200 end, label = "Description" },
		{ width = 70, label = "Num", sortable = true },
	}
	self.colLabels = true
	self.label = "^7Columns:"
	return self
end

function TestList:GetRowValue(column, index, value)
	if column == 1 then
		return value.name
	elseif column == 2 then
		return value.desc
	end
	return "^7" .. value.num
end

function TestList:AddValueTooltip(tooltip, index, value)
	tooltip:Clear()
	tooltip:AddLine(16, "^7" .. value.name)
	tooltip:AddLine(14, "^8Row " .. index .. ": " .. value.desc)
end

function TestList:ReSort(colIndex)
	if colIndex == 1 then
		table.sort(self.list, function(a, b) return a.name < b.name end)
	else
		table.sort(self.list, function(a, b) return a.num > b.num end)
	end
	self.lastEvent = "sort " .. colIndex
end

function TestList:OnSelect(index, value)
	self.lastEvent = "select " .. index
end

function TestList:OnSelClick(index, value, doubleClick)
	self.lastEvent = "click " .. index .. (doubleClick and " double" or "")
end

function TestList:OnSelCopy(index, value)
	self.lastEvent = "copy " .. value.name
end

function TestList:OnSelDelete(index, value)
	t_remove(self.list, index)
	self.selIndex = nil
	self.selValue = nil
	self.lastEvent = "delete " .. index
end

function TestList:OnSelKeyDown(index, value, key)
	self.lastEvent = "key " .. key
end

local G = { }

function G:Init()
	self.host = new("ControlHost"):ControlHost()
	local c = self.host.controls
	local rows = { }
	local names = { "Fireball", "Arc", "Cyclone", "Blade Vortex", "Zombie", "Frostbolt", "Ice Nova", "Spark", "Vaal Haste", "Herald of Ice", "Molten Strike", "Earthquake", "Tornado Shot", "Lightning Arrow" }
	for i, name in ipairs(names) do
		t_insert(rows, { name = name, desc = "Description of " .. name .. (i % 3 == 0 and ", a much longer text that will not fit into its column at all" or ""), num = (i * 37) % 23 })
	end
	c.cols = new("U12TestList"):U12TestList(nil, {20, 40, 520, 200}, rows)
	self.icon = NewImageHandle()
	self.icon:Load("Assets/icon_ring_left.png")
	local plainRows = { }
	for i = 1, 24 do
		t_insert(plainRows, { label = "Plain row " .. i })
	end
	c.plain = new("ListControl"):ListControl(nil, {580, 40, 260, 170}, 20, "VERTICAL", false, plainRows)
	c.plain.showRowSeparators = true
	c.plain.GetRowValue = function(control, column, index, value)
		return "^7" .. value.label
	end
	c.plain.GetRowIcon = function(control, column, index, value)
		if index % 2 == 0 then
			return self.icon
		end
	end
	c.plain.OverrideSelectIndex = function(control, index)
		if IsKeyDown("SHIFT") and control.selIndex then
			control.highlightIndex = index
			return true
		else
			control.highlightIndex = nil
		end
		return false
	end
	c.plain.SetHighlightColor = function(control, index, value)
		if not control.highlightIndex or not control.selIndex then
			return false
		end
		if math.min(control.selIndex, control.highlightIndex) <= index and math.max(control.selIndex, control.highlightIndex) >= index then
			if control.selIndex == index or control.highlightIndex == index then
				SetDrawColor(1, 0.5, 0)
			else
				SetDrawColor(1, 1, 0)
			end
			return true
		end
		return false
	end
	c.plain.OnSelClick = function(control, index, value, doubleClick)
		control.lastEvent = "plain click " .. index
		if doubleClick then
			return false
		end
	end
	c.noScroll = new("ListControl"):ListControl(nil, {580, 240, 260, 70}, 16, false, false, { { label = "Fixed one" }, { label = "Fixed two" } })
	c.noScroll.GetRowValue = function(control, column, index, value)
		return value.label
	end
	c.empty = new("ListControl"):ListControl(nil, {20, 280, 520, 60}, 16, "VERTICAL", false)
	c.empty.defaultText = "^x7F7F7FThis list is empty.\nIts default text has two lines."
	c.status = new("LabelControl"):LabelControl(nil, {20, 360, 0, 16}, function()
		return string.format("^7cols: %s sel %s | plain: %s sel %s hl %s", c.cols.lastEvent or "-", tostring(c.cols.selIndex), c.plain.lastEvent or "-", tostring(c.plain.selIndex), tostring(c.plain.highlightIndex))
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
