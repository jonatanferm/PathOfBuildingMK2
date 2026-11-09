-- Gallery "u9_check" (U9): CheckBoxControl with the label on the left, on the right
-- (labelRight), no label, an empty label, disabled, a borderFunc, a function label and an
-- anchored box without changeFunc; the clickable area includes the label; DrawCheckMark.
-- Rust counterpart: src/gallery/u9_check.rs. Interactions: u9_check.pobscript.

local G = { }

function G:Init()
	self.host = new("ControlHost"):ControlHost()
	local c = self.host.controls
	self.changes = 0
	self.last = "none"
	local function changed(state)
		self.changes = self.changes + 1
		self.last = tostring(state)
	end
	c.left = new("CheckBoxControl"):CheckBoxControl(nil, {200, 20, 18}, "Label left:", changed)
	c.right = new("CheckBoxControl"):CheckBoxControl(nil, {200, 50, 20}, "^7Label right", changed, nil, true)
	c.right.labelRight = true
	c.nolabel = new("CheckBoxControl"):CheckBoxControl(nil, {200, 80, 18}, nil, changed)
	c.empty = new("CheckBoxControl"):CheckBoxControl(nil, {260, 80, 18}, "", changed, nil, true)
	c.disabled = new("CheckBoxControl"):CheckBoxControl(nil, {200, 110, 18}, "Disabled:", changed, nil, true) {
		enabled = false,
	}
	c.disabledOff = new("CheckBoxControl"):CheckBoxControl(nil, {260, 110, 18}, nil, changed) {
		enabled = false,
	}
	c.border = new("CheckBoxControl"):CheckBoxControl(nil, {200, 140, 18}, "Border:", changed)
	c.border.borderFunc = function()
		if self.changes % 2 == 1 then
			return 0.753, 0.502, 0.502
		end
		return 0.451, 0.576, 0.702
	end
	c.fnLabel = new("CheckBoxControl"):CheckBoxControl(nil, {200, 170, 18}, "Fn label:", changed)
	c.fnLabel.label = function()
		return "Changes " .. self.changes .. ":"
	end
	c.anchored = new("CheckBoxControl"):CheckBoxControl({"LEFT", c.left, "RIGHT"}, {150, 0, 18}, "Anchored:")
	c.status = new("LabelControl"):LabelControl(nil, {20, 220, 0, 16}, function()
		return "changes " .. self.changes .. " last " .. self.last
	end)
end

function G:OnFrame(inputEvents, viewPort)
	self.host:ProcessControlsInput(inputEvents, viewPort)
	SetDrawColor(0.1, 0.1, 0.1)
	DrawImage(nil, viewPort.x, viewPort.y, viewPort.width, viewPort.height)
	self.host:DrawControls(viewPort)
end

return G
