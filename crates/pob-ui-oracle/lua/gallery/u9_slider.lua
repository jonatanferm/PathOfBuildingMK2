-- Gallery "u9_slider" (U9): SliderControl plain and with divCount (steps: dividers and the
-- arrow knob, GetDivVal), knob drag (clamped past the end), slide clicks, the wheel with
-- SHIFT/CTRL/default speeds, the arrow keys over a hovered slider, a custom
-- scrollWheelSpeedTbl, a disabled slider.
-- Rust counterpart: src/gallery/u9_slider.rs. Interactions: u9_slider.pobscript.

local G = { }

function G:Init()
	self.host = new("ControlHost"):ControlHost()
	local c = self.host.controls
	self.changes = 0
	self.lastVal = 0
	local function changed(val)
		self.changes = self.changes + 1
		self.lastVal = val
	end
	c.plain = new("SliderControl"):SliderControl(nil, {20, 20, 200, 18}, changed)
	c.div = new("SliderControl"):SliderControl(nil, {20, 60, 300, 16}, changed)
	c.div.divCount = 4
	c.custom = new("SliderControl"):SliderControl(nil, {20, 100, 150, 20}, nil, { ["SHIFT"] = 0.2, ["CTRL"] = 0.002, ["DEFAULT"] = 0.02 })
	c.disabled = new("SliderControl"):SliderControl(nil, {20, 140, 100, 18}, changed) {
		enabled = false,
	}
	c.disabled.val = 0.5
	c.status = new("LabelControl"):LabelControl(nil, {20, 200, 0, 16}, function()
		local idx, frac = c.div:GetDivVal()
		return string.format("changes %d  val %.4f  div %d %.4f  custom %.4f", self.changes, self.lastVal, idx, frac, c.custom.val)
	end)
end

function G:OnFrame(inputEvents, viewPort)
	self.host:ProcessControlsInput(inputEvents, viewPort)
	SetDrawColor(0.1, 0.1, 0.1)
	DrawImage(nil, viewPort.x, viewPort.y, viewPort.width, viewPort.height)
	self.host:DrawControls(viewPort)
end

return G
