-- Gallery "u9_scroll" (U9): ScrollBarControl vertical (default dir) and horizontal; arrow
-- clicks, the hold-to-repeat timer (with its pause when the cursor leaves the arrow), knob drag,
-- slide clicks, wheel and PAGEUP/PAGEDOWN, autoHide with the default step (width * 2), a
-- disabled bar (content fits), a locked bar, ScrollIntoView; DrawArrow in all four directions.
-- Rust counterpart: src/gallery/u9_scroll.rs. Interactions: u9_scroll.pobscript.

local G = { }

function G:Init()
	self.host = new("ControlHost"):ControlHost()
	local c = self.host.controls
	c.v = new("ScrollBarControl"):ScrollBarControl(nil, {300, 20, 18, 300}, 40)
	c.h = new("ScrollBarControl"):ScrollBarControl(nil, {20, 340, 400, 16}, 30, "HORIZONTAL")
	c.auto = new("ScrollBarControl"):ScrollBarControl(nil, {340, 20, 14, 200}, nil, "VERTICAL", true)
	c.off = new("ScrollBarControl"):ScrollBarControl(nil, {380, 20, 18, 200}, 20)
	c.locked = new("ScrollBarControl"):ScrollBarControl(nil, {420, 20, 18, 200}, 20)
	c.locked.locked = true
	c.intoView = new("ButtonControl"):ButtonControl(nil, {20, 400, 100, 20}, "Into view", function()
		c.v:ScrollIntoView(600, 50)
	end)
	c.status = new("LabelControl"):LabelControl(nil, {20, 370, 0, 16}, function()
		return string.format("v %d  h %d  auto %d (%s)  locked %d", c.v.offset, c.h.offset, c.auto.offset, tostring(c.auto.enabled), c.locked.offset)
	end)
end

function G:OnFrame(inputEvents, viewPort)
	local c = self.host.controls
	c.v:SetContentDimension(1000, 300)
	c.h:SetContentDimension(1200, 400)
	c.auto:SetContentDimension(c.v.offset > 400 and 100 or 500, 200)
	c.off:SetContentDimension(100, 200)
	c.locked:SetContentDimension(800, 200)
	self.host:ProcessControlsInput(inputEvents, viewPort)
	SetDrawColor(0.1, 0.1, 0.1)
	DrawImage(nil, viewPort.x, viewPort.y, viewPort.width, viewPort.height)
	self.host:DrawControls(viewPort)
end

return G
