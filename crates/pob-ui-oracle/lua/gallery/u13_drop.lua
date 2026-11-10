-- Gallery "u13_drop" (U13): DropDownControl opening, the dropped list at sublayer 5 with its
-- scroll bar, the hovered row, BODY/OUT/HOVER tooltips through tooltipFunc and the plain
-- tooltipText path, selFunc timing (click, wheel, UP/DOWN over a closed dropdown, keys while
-- dropped), RIGHTBUTTON, ESCAPE, a disabled dropdown, borderFunc, dropUp at the screen bottom, and
-- in a 200 px high sub-viewport a list clipped below and one that drops up and is clipped above.
-- Rust counterpart: src/gallery/u13_drop.rs. Interactions: u13_drop.pobscript.

local t_insert = table.insert

local G = { }

local function options(prefix, n)
	local list = { }
	for i = 1, n do
		t_insert(list, (i % 7 == 0 and "^xE05030" or "") .. prefix .. " " .. i)
	end
	return list
end

function G:Init()
	self.host = new("ControlHost"):ControlHost()
	self.host2 = new("ControlHost"):ControlHost()
	local c = self.host.controls
	local c2 = self.host2.controls
	local status = "-"
	local function selFunc(name)
		return function(index, value)
			status = name .. " " .. index .. " " .. tostring(value)
		end
	end
	local function tooltipFunc(tooltip, mode, index, value)
		tooltip:Clear()
		tooltip:AddLine(14, "^7" .. mode .. " " .. tostring(index) .. " " .. tostring(value))
	end
	c.basic = new("DropDownControl"):DropDownControl(nil, {20, 40, 200, 20}, options("Option", 30), selFunc("basic"))
	c.basic.tooltipFunc = tooltipFunc
	c.text = new("DropDownControl"):DropDownControl(nil, {260, 40, 200, 20}, options("Text", 5), selFunc("text"), "^7A plain tooltip text")
	c.disabled = new("DropDownControl"):DropDownControl(nil, {500, 40, 160, 20}, options("Off", 3), selFunc("disabled"), "^7never shown")
	c.disabled.enabled = false
	c.disabled.selIndex = 2
	c.border = new("DropDownControl"):DropDownControl(nil, {700, 40, 160, 20}, options("Border", 4), selFunc("border"))
	c.border.borderFunc = function()
		return 0.8, 0.3, 0.1
	end
	c.bottom = new("DropDownControl"):DropDownControl(nil, {20, 680, 200, 20}, options("Bottom", 10), selFunc("bottom"))
	c.bottom.tooltipFunc = tooltipFunc
	c.status = new("LabelControl"):LabelControl(nil, {20, 10, 0, 16}, function()
		return "^7" .. status
	end)
	c2.clipBelow = new("DropDownControl"):DropDownControl(nil, {910, 430, 200, 20}, options("Below", 30), selFunc("clipBelow"))
	c2.clipAbove = new("DropDownControl"):DropDownControl(nil, {910, 590, 200, 20}, options("Above", 30), selFunc("clipAbove"))
	c2.clipAbove.tooltipFunc = tooltipFunc
	self.vp2 = { x = 900, y = 420, width = 360, height = 200 }
end

function G:OnFrame(inputEvents, viewPort)
	self.host:ProcessControlsInput(inputEvents, viewPort)
	self.host2:ProcessControlsInput(inputEvents, self.vp2)
	SetDrawColor(0.1, 0.1, 0.1)
	DrawImage(nil, viewPort.x, viewPort.y, viewPort.width, viewPort.height)
	SetDrawColor(0.2, 0.2, 0.3)
	DrawImage(nil, self.vp2.x, self.vp2.y, self.vp2.width, self.vp2.height)
	self.host:DrawControls(viewPort)
	self.host2:DrawControls(self.vp2)
end

return G
