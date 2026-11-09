-- Gallery "smoke" (U8): the oracle's own end-to-end check of the gallery harness. It draws
-- directly through the SimpleGraphic API (no ported control is needed on the Rust side):
-- layer and sublayer switches, a viewport, plain and escape colours (including the DrawString
-- escape leak into a later DrawImage), DrawImageQuad, string width and alignment, the cursor
-- position, and input events (clicks, double clicks, typed characters, the wheel).
-- Rust counterpart: src/gallery/smoke.rs. Interactions: smoke.pobscript.

local G = { }

function G:Init()
	self.host = new("ControlHost"):ControlHost()
	self.typed = ""
	self.clicks = 0
	self.doubleClicks = 0
	self.wheel = 0
end

function G:OnFrame(inputEvents, viewPort)
	for id, event in ipairs(inputEvents) do
		if event.type == "Char" then
			self.typed = self.typed .. event.key
		elseif event.type == "KeyDown" then
			if event.key == "LEFTBUTTON" then
				self.clicks = self.clicks + 1
				if event.doubleClick then
					self.doubleClicks = self.doubleClicks + 1
				end
			elseif event.key == "WHEELUP" then
				self.wheel = self.wheel + 1
			elseif event.key == "WHEELDOWN" then
				self.wheel = self.wheel - 1
			end
		end
	end
	self.host:ProcessControlsInput(inputEvents, viewPort)

	SetDrawColor(0.1, 0.1, 0.1)
	DrawImage(nil, viewPort.x, viewPort.y, viewPort.width, viewPort.height)
	SetDrawLayer(nil, 5)
	SetDrawColor(1, 1, 1)
	DrawString(10, 10, "LEFT", 16, "VAR", "^7Smoke ^xFF8000gallery")
	local width = DrawStringWidth(16, "VAR", "Smoke gallery")
	DrawImage(nil, 10, 30, width, 2)
	SetDrawColor("^3")
	DrawString(viewPort.width / 2, 40, "CENTER_X", 14, "VAR BOLD", string.format("clicks %d (double %d)  wheel %d  typed '%s'", self.clicks, self.doubleClicks, self.wheel, self.typed))
	DrawString(0, 60, "RIGHT", 12, "FIXED", "right aligned")
	local cursorX, cursorY = GetCursorPos()
	SetViewport(100, 100, 200, 100)
	SetDrawColor(0, 1, 0, 0.5)
	DrawImage(nil, cursorX - 104, cursorY - 104, 8, 8)
	DrawImageQuad(nil, 0, 0, 50, 10, 60, 60, 5, 40)
	SetViewport()
	SetDrawLayer(2)
	SetDrawColor(1, 0, 0)
	DrawImage(nil, viewPort.width - 20, viewPort.height - 20, 10, 10, 0, 0, 1, 1)
	SetDrawLayer(0)
	self.host:DrawControls(viewPort)
end

return G
