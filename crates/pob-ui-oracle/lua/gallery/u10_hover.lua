-- Gallery "u10_hover" (U10): hover tooltips through TooltipHost (ButtonControl, SliderControl).
-- Plain lines (tooltipText, with "\n" and leaking colour escapes), separators (leading,
-- back-to-back, trailing, between text), a string colour and centred lines, `maxWidth` wrap,
-- struck-through (disabled) mod lines, the CheckForUpdate memo (rebuild count, also forced by
-- main.notSupportedModTooltips), placement at all four edges (right flip, left clamp with the
-- move below the box, bottom flip, top clamp of a tall tooltip), and a `tooltip.Draw` override as
-- TreeTab.lua:1748-1755 installs it.
-- Rust counterpart: src/gallery/u10_hover.rs. Interactions: u10_hover.pobscript.

local G = { }

function G:Init()
	self.host = new("ControlHost"):ControlHost()
	local c = self.host.controls
	self.memoKey = 1
	self.rebuilds = 0
	c.plain = new("ButtonControl"):ButtonControl(nil, {20, 20, 100, 20}, "Plain", function() end)
	c.plain.tooltipText = "^7A plain tooltip line"
	c.multi = new("ButtonControl"):ButtonControl(nil, {130, 20, 100, 20}, "Multi", function() end)
	c.multi.tooltipText = function()
		return "^7First line\n^1Second line, red\nthird line keeps the red\n\n^xFFFF00after an empty line"
	end
	c.memo = new("ButtonControl"):ButtonControl(nil, {240, 20, 100, 20}, "Memo", function()
		self.memoKey = self.memoKey + 1
	end)
	c.memo.tooltipFunc = function(tooltip)
		if tooltip:CheckForUpdate(self.memoKey, "const", nil, true) then
			self.rebuilds = self.rebuilds + 1
			tooltip:AddLine(16, "^7Rebuilt " .. self.rebuilds .. " times")
			tooltip:AddSeparator(10)
			tooltip:AddLine(14, "^7key " .. self.memoKey)
		end
	end
	c.nsmt = new("ButtonControl"):ButtonControl(nil, {350, 20, 100, 20}, "NSMT", function()
		main.notSupportedModTooltips = not main.notSupportedModTooltips
	end)
	c.seps = new("ButtonControl"):ButtonControl(nil, {460, 20, 100, 20}, "Seps", function() end)
	c.seps.tooltipFunc = function(tooltip)
		tooltip:Clear()
		tooltip.color = "^x1AA29B"
		tooltip:AddSeparator()
		tooltip:AddLine(16, "^7Left aligned")
		tooltip:AddSeparator(14)
		tooltip:AddSeparator(10)
		tooltip.center = true
		tooltip:AddLine(16, "^7Centred line")
		tooltip:AddLine(12, "^8a shorter centred line")
		tooltip:AddSeparator(6)
		tooltip.center = false
		tooltip:AddLine(14, "^7Left again")
		tooltip:AddSeparator()
	end
	c.wrap = new("ButtonControl"):ButtonControl(nil, {570, 20, 100, 20}, "Wrap", function() end)
	c.wrap.tooltipFunc = function(tooltip)
		tooltip:Clear()
		tooltip.maxWidth = 220
		tooltip:AddLine(16, "^7This rather long line is wrapped by main:WrapString at the tooltip's maximum width minus the padding.")
		tooltip:AddLine(14, "short")
		tooltip:AddLine(14, "^xAAAAFFAnotherveryveryverylongwordwithoutanyspaces then words")
	end
	c.strike = new("ButtonControl"):ButtonControl(nil, {680, 20, 100, 20}, "Strike", function() end)
	c.strike.tooltipFunc = function(tooltip)
		tooltip:Clear()
		tooltip:AddLine(16, "^7Enabled mod line", "FONTIN SC", { disabled = false })
		tooltip:AddLine(16, "^8Disabled mod line", "FONTIN SC", { disabled = true })
		tooltip.center = true
		tooltip:AddLine(16, "^8Disabled, centred", "FONTIN", { disabled = true })
		tooltip:AddLine(14, "^7no mod line")
	end
	-- placement at the four edges
	local function edgeLines(tooltip, n)
		tooltip:Clear()
		for i = 1, n do
			tooltip:AddLine(16, "^7Edge tooltip line " .. i)
		end
	end
	c.right = new("ButtonControl"):ButtonControl(nil, {function() return main.screenW - 70 end, 60, 60, 20}, "Right", function() end)
	c.right.tooltipFunc = function(tooltip)
		edgeLines(tooltip, 3)
	end
	c.wide = new("ButtonControl"):ButtonControl(nil, {300, 60, 60, 20}, "Wide", function() end)
	c.wide.tooltipFunc = function(tooltip)
		tooltip:Clear()
		tooltip:AddLine(20, "^7" .. string.rep("A very wide tooltip line. ", 6))
		tooltip:AddLine(16, "^7It flips left, clamps at the viewport and moves below the button.")
	end
	c.bottom = new("ButtonControl"):ButtonControl(nil, {20, function() return main.screenH - 30 end, 60, 20}, "Bottom", function() end)
	c.bottom.tooltipFunc = function(tooltip)
		edgeLines(tooltip, 6)
	end
	c.corner = new("ButtonControl"):ButtonControl(nil, {function() return main.screenW - 70 end, function() return main.screenH - 30 end, 60, 20}, "Corner", function() end)
	c.corner.tooltipFunc = function(tooltip)
		edgeLines(tooltip, 6)
	end
	c.tall = new("ButtonControl"):ButtonControl(nil, {500, function() return main.screenH - 60 end, 60, 20}, "Tall", function() end)
	c.tall.tooltipFunc = function(tooltip)
		edgeLines(tooltip, 60)
	end
	-- tooltip.Draw override (TreeTab.lua:1741-1755)
	c.slider = new("SliderControl"):SliderControl(nil, {20, 120, 200, 18}, function(value) end)
	c.slider.tooltipFunc = function(tooltip, val)
		tooltip:Clear()
		if not c.slider.dragging then
			tooltip:AddLine(16, "^7This tooltip is drawn by an overridden Draw")
		end
	end
	c.slider.tooltip.realDraw = c.slider.tooltip.Draw
	c.slider.tooltip.Draw = function(self, x, y, width, height, viewPort)
		local sliderOffsetX = round(184 * (1 - c.slider.val))
		local tooltipWidth, tooltipHeight = self:GetSize()
		if main.screenW >= 1338 - sliderOffsetX then
			return c.slider.tooltip.realDraw(self, x - 8 - sliderOffsetX, y - 4 - tooltipHeight, width, height, viewPort)
		end
		return c.slider.tooltip.realDraw(self, x, y, width, height, viewPort)
	end
	c.status = new("LabelControl"):LabelControl(nil, {20, 160, 0, 16}, function()
		return "^7rebuilds " .. self.rebuilds .. "  key " .. self.memoKey .. "  nsmt " .. tostring(main.notSupportedModTooltips)
	end)
end

function G:OnFrame(inputEvents, viewPort)
	self.host:ProcessControlsInput(inputEvents, viewPort)
	SetDrawColor(0.1, 0.1, 0.1)
	DrawImage(nil, viewPort.x, viewPort.y, viewPort.width, viewPort.height)
	self.host:DrawControls(viewPort)
end

return G
