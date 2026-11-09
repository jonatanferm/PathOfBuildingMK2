-- Gallery "u9_misc" (U9): SectionControl (string and function label), LabelControl (its width
-- function anchors a neighbour), RectangleOutlineControl (default and custom colours/stroke),
-- DraggerControl (the "//", "+", "-", "x" glyphs, text with overSizeText, image, locked,
-- disabled; drag down/up callbacks with the cursor table, right click, onHover), and the
-- main draw helpers DrawArrow (four directions and an unknown one), DrawCheckMark,
-- RenderCircle, RenderRing and WorldToScreen.
-- Rust counterpart: src/gallery/u9_misc.rs. Interactions: u9_misc.pobscript.

local G = { }

function G:Init()
	self.host = new("ControlHost"):ControlHost()
	local c = self.host.controls
	self.downs = 0
	self.lastDown = "-"
	self.lastUp = "-"
	self.rights = 0
	self.hovers = 0
	c.section = new("SectionControl"):SectionControl(nil, {20, 30, 300, 120}, "Section label")
	c.sectionFn = new("SectionControl"):SectionControl({"TOPLEFT", c.section, "BOTTOMLEFT"}, {0, 20, 300, 60}, function()
		return "Downs " .. self.downs
	end)
	c.label = new("LabelControl"):LabelControl({"TOPLEFT", c.section, "TOPLEFT"}, {10, 14, 0, 16}, "^7Label in section")
	c.labelBig = new("LabelControl"):LabelControl({"TOPLEFT", c.label, "BOTTOMLEFT"}, {0, 4, 0, 20}, "^xFFD700Bigger")
	c.labelNext = new("LabelControl"):LabelControl({"LEFT", c.labelBig, "RIGHT"}, {6, 0, 0, 12}, "^8next to it")
	c.outline = new("RectangleOutlineControl"):RectangleOutlineControl(nil, {360, 30, 100, 60})
	c.outline2 = new("RectangleOutlineControl"):RectangleOutlineControl(nil, {480, 30, 100, 60}, {1, 0.5, 0}, 3)
	c.dragger = new("DraggerControl"):DraggerControl(nil, {360, 120, 14, 14}, "//", function(pos)
		self.downs = self.downs + 1
		self.lastDown = pos.X .. "," .. pos.Y
	end, function(delta)
		self.lastUp = delta.X .. "," .. delta.Y
	end, function(pos)
		self.rights = self.rights + 1
	end, function()
		self.hovers = self.hovers + 1
	end) {
		-- a dragger keeps the focus (OnKeyDown/OnKeyUp return self); hiding it releases it
		shown = function()
			return self.rights == 0
		end,
	}
	c.draggerPlus = new("DraggerControl"):DraggerControl(nil, {400, 120, 20, 20}, "+")
	c.draggerMinus = new("DraggerControl"):DraggerControl(nil, {430, 120, 20, 20}, "-")
	c.draggerX = new("DraggerControl"):DraggerControl(nil, {460, 120, 20, 20}, "x")
	c.draggerText = new("DraggerControl"):DraggerControl(nil, {490, 120, 60, 20}, "Drag")
	c.draggerText.overSizeText = 2
	c.draggerLocked = new("DraggerControl"):DraggerControl(nil, {560, 120, 60, 20}, "Locked")
	c.draggerLocked.locked = true
	c.draggerOff = new("DraggerControl"):DraggerControl(nil, {630, 120, 60, 20}, "Off") {
		enabled = false,
	}
	c.draggerImg = new("DraggerControl"):DraggerControl(nil, {700, 110, 40, 40}, "")
	c.draggerImg:SetImage("Assets/icon_helmet.png")
	c.status = new("LabelControl"):LabelControl(nil, {20, 260, 0, 16}, function()
		return string.format("downs %d  down %s  up %s  rights %d  hovers %d", self.downs, self.lastDown, self.lastUp, self.rights, self.hovers)
	end)
end

function G:OnFrame(inputEvents, viewPort)
	self.host:ProcessControlsInput(inputEvents, viewPort)
	SetDrawColor(0.1, 0.1, 0.1)
	DrawImage(nil, viewPort.x, viewPort.y, viewPort.width, viewPort.height)
	SetDrawColor(1, 1, 1)
	main:DrawArrow(40, 320, 20, 20, "UP")
	main:DrawArrow(70, 320, 20, 20, "RIGHT")
	main:DrawArrow(100, 320, 20, 14, "DOWN")
	main:DrawArrow(130, 320, 14, 20, "LEFT")
	main:DrawArrow(160, 320, 20, 20, "NONE")
	SetDrawColor(0, 1, 0)
	main:DrawCheckMark(200, 320, 16)
	main:DrawCheckMark(230, 320, 24 * 0.8)
	SetDrawColor(0.8, 0.2, 0.2)
	main:RenderCircle(300, 280, 200, 120, 0, 0, 50)
	SetDrawColor(0.2, 0.6, 1)
	main:RenderRing(520, 280, 200, 120, 10, -20, 40, 2)
	local sx, sy = main:WorldToScreen(30, 40, 5, 200, 120)
	SetDrawColor(1, 1, 1)
	DrawString(20, 420, "LEFT", 14, "VAR", "w2s " .. sx .. " " .. sy)
	self.host:DrawControls(viewPort)
end

return G
