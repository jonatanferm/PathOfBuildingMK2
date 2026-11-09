-- Gallery "u9_button" (U9): ButtonControl in its states (idle, hover, pressed, released,
-- disabled, locked as a value and as a function, image, the "+", "-" and "x" glyphs,
-- overSizeText, onHover, enterFunc) and LabelControl with a function label.
-- Rust counterpart: src/gallery/u9_button.rs. Interactions: u9_button.pobscript.

local G = { }

function G:Init()
	self.host = new("ControlHost"):ControlHost()
	local c = self.host.controls
	self.clicks = 0
	self.enters = 0
	self.hovers = 0
	c.plain = new("ButtonControl"):ButtonControl(nil, {20, 20, 120, 24}, "Plain", function()
		self.clicks = self.clicks + 1
	end)
	c.plus = new("ButtonControl"):ButtonControl({"LEFT", c.plain, "RIGHT"}, {8, 0, 24, 24}, "+", function()
		self.clicks = self.clicks + 10
	end)
	c.minus = new("ButtonControl"):ButtonControl({"LEFT", c.plus, "RIGHT"}, {8, 0, 24, 24}, "-", function()
		self.clicks = self.clicks - 1
	end)
	c.close = new("ButtonControl"):ButtonControl({"LEFT", c.minus, "RIGHT"}, {8, 0, 20, 20}, "x", function()
		self.clicks = self.clicks + 100
	end)
	c.disabled = new("ButtonControl"):ButtonControl(nil, {20, 60, 120, 24}, "Disabled", function()
		self.clicks = self.clicks + 1000
	end) {
		enabled = false,
	}
	c.locked = new("ButtonControl"):ButtonControl(nil, {160, 60, 120, 24}, "Locked", function()
		self.clicks = self.clicks + 1
	end)
	c.locked.locked = true
	c.lockedFn = new("ButtonControl"):ButtonControl(nil, {300, 60, 120, 24}, "Locked odd", function()
		self.clicks = self.clicks + 1
	end) {
		locked = function()
			return self.clicks % 2 == 1
		end,
	}
	c.image = new("ButtonControl"):ButtonControl(nil, {20, 100, 40, 40}, "", function()
		self.clicks = self.clicks + 1
	end)
	c.image:SetImage("Assets/icon_helmet.png")
	c.imageOff = new("ButtonControl"):ButtonControl(nil, {70, 100, 40, 40}, "", function() end) {
		enabled = false,
	}
	c.imageOff:SetImage("Assets/icon_helmet.png")
	c.over = new("ButtonControl"):ButtonControl(nil, {130, 100, 120, 24}, "Oversize", function()
		self.clicks = self.clicks + 1
	end)
	c.over.overSizeText = 3
	c.hover = new("ButtonControl"):ButtonControl(nil, {270, 100, 120, 24}, "Hover me", function()
		self.clicks = self.clicks + 1
	end, function()
		self.hovers = self.hovers + 1
	end)
	c.enter = new("ButtonControl"):ButtonControl(nil, {410, 100, 120, 24}, function()
		return "Enter " .. self.enters
	end, function()
		self.clicks = self.clicks + 1
	end)
	c.enter.enterFunc = function()
		self.enters = self.enters + 1
	end
	c.hidden = new("ButtonControl"):ButtonControl(nil, {550, 100, 120, 24}, "Hidden", function()
		self.clicks = self.clicks + 1
	end) {
		shown = function()
			return self.clicks < 20
		end,
	}
	c.status = new("LabelControl"):LabelControl(nil, {20, 160, 0, 16}, function()
		return string.format("^7clicks %d  enters %d  hovers %d", self.clicks, self.enters, self.hovers)
	end)
	c.statusRight = new("LabelControl"):LabelControl({"LEFT", c.status, "RIGHT"}, {10, 0, 0, 14}, "^xFF8000after")
end

function G:OnFrame(inputEvents, viewPort)
	self.host:ProcessControlsInput(inputEvents, viewPort)
	SetDrawColor(0.1, 0.1, 0.1)
	DrawImage(nil, viewPort.x, viewPort.y, viewPort.width, viewPort.height)
	self.host:DrawControls(viewPort)
end

return G
