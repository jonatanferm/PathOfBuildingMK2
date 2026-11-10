-- Gallery "u13_slot" (U13): a DropDownControl subclass in the ItemSlotControl pattern (class
-- CanReceiveDrag/ReceiveDrag, an own Draw that calls `self.DropDownControl:Draw(viewPort)` and
-- then `self:DrawControls(viewPort)`, an own OnKeyDown calling the super method, a tooltipFunc
-- reading `self.dropped`) and plain dropdowns as ListControl drag targets: one with instance
-- CanReceiveDrag/ReceiveDrag (Build.lua mainSkillMinion style) and one without (always armed).
-- An armed target draws the green otherDragSource overlay (DropDownControl.lua:290-293).
-- Rust counterpart: src/gallery/u13_slot.rs. Interactions: u13_slot.pobscript.

local t_insert = table.insert

local SlotClass = newClass("U13Slot", "DropDownControl")

function SlotClass:U13Slot(anchor, rect, list, label)
	self:DropDownControl(anchor, rect, list, function(index, value)
		self.lastEvent = "sel " .. index .. " " .. tostring(value)
	end)
	self.label = label
	self.tooltipFunc = function(tooltip, mode, index, value)
		tooltip:Clear()
		tooltip:AddLine(14, "^7slot " .. mode .. " " .. tostring(index) .. (self.dropped and " dropped" or ""))
	end
	return self
end

function SlotClass:CanReceiveDrag(type, value)
	return type == "Entry" and value.ok
end

function SlotClass:ReceiveDrag(type, value, source)
	t_insert(self.list, value.label)
	self.selIndex = #self.list
	self.lastEvent = "got " .. value.label
end

function SlotClass:Draw(viewPort, noTooltip)
	local x, y = self:GetPos()
	local width, height = self:GetSize()
	DrawString(x - 2, y + 2, "RIGHT_X", height - 4, "VAR", "^7" .. self.label .. ":")
	self.DropDownControl:Draw(viewPort)
	self:DrawControls(viewPort)
	if self:IsMouseOver() and self.otherDragSource then
		SetDrawColor(0.2, 0.2, 1, 0.5)
		DrawImage(nil, x, y + height, width, 4)
	end
end

function SlotClass:OnKeyDown(key)
	if not self:IsShown() or not self:IsEnabled() then
		return
	end
	return self.DropDownControl:OnKeyDown(key)
end

local G = { }

function G:Init()
	self.host = new("ControlHost"):ControlHost()
	local c = self.host.controls
	local minionEvent = "-"
	local rows = { }
	for i = 1, 6 do
		t_insert(rows, { label = "Gem " .. i, ok = i % 2 == 1 })
	end
	c.src = new("ListControl"):ListControl(nil, {20, 40, 200, 200}, 16, "VERTICAL", false, rows)
	c.src.GetRowValue = function(control, column, index, value)
		return "^7" .. value.label .. (value.ok and "" or " (no)")
	end
	c.src.GetDragValue = function(control, index, value)
		return "Entry", value
	end
	c.slot = new("U13Slot"):U13Slot(nil, {360, 40, 200, 20}, { "None" }, "Slot")
	c.minion = new("DropDownControl"):DropDownControl(nil, {360, 120, 200, 20}, { "Minion A", "Minion B" }, function(index, value)
		minionEvent = "minion sel " .. index
	end)
	function c.minion.CanReceiveDrag(control, type, value)
		return value.ok == false
	end
	function c.minion.ReceiveDrag(control, type, value, source)
		minionEvent = "minion got " .. value.label .. " at " .. control.selIndex
	end
	c.plain = new("DropDownControl"):DropDownControl(nil, {360, 200, 200, 20}, { "Plain" })
	c.src.dragTargetList = { c.slot, c.minion, c.plain }
	c.status = new("LabelControl"):LabelControl(nil, {20, 10, 0, 16}, function()
		return "^7" .. tostring(c.slot.lastEvent) .. " | " .. minionEvent
	end)
end

function G:OnFrame(inputEvents, viewPort)
	self.host:ProcessControlsInput(inputEvents, viewPort)
	SetDrawColor(0.1, 0.1, 0.1)
	DrawImage(nil, viewPort.x, viewPort.y, viewPort.width, viewPort.height)
	self.host:DrawControls(viewPort)
end

return G
