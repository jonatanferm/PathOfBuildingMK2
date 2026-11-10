-- Gallery "u12_text" (U12): TextListControl and PathControl.
-- TextListControl: the side-bar stat box shape (RIGHT_X/LEFT columns, per-row x/align/font
-- overrides, underlined cells, spacer rows), the hovered line, onClick, the wheel, Shift+wheel
-- section jumps (sectionHeights) and the jump to offsetMax, and a list with the default column.
-- PathControl: breadcrumb buttons for a base path with and without a folder name, clicking a
-- breadcrumb (onChange), SetSubPath deeper and shorter (buttons hidden, then reused), Undo/Redo.
-- Rust counterpart: src/gallery/u12_text.rs. Interactions: u12_text.pobscript.

local t_insert = table.insert

local G = { }

function G:Init()
	self.host = new("ControlHost"):ControlHost()
	local c = self.host.controls
	local list = { }
	for s = 1, 3 do
		t_insert(list, { height = 18, "^7Section " .. s .. ":" })
		for i = 1, 5 do
			t_insert(list, { height = 16, "^7Stat " .. s .. "." .. i .. ":", "^x33FF77" .. (s * 100 + i * 7), underline = { false, i % 2 == 1 } })
		end
		t_insert(list, { height = 14, align = "CENTER_X", x = 140, "^8a centred note for section " .. s })
		t_insert(list, { height = 16, font = "FIXED", "^7Fixed:", "^7" .. s })
		t_insert(list, { height = 6 })
	end
	c.text = new("TextListControl"):TextListControl(nil, {20, 40, 320, 200}, {{x=170,align="RIGHT_X"},{x=174,align="LEFT"}}, list, {0, 136, 272})
	function c.text.onClick(hoveredLine)
		self.clicked = hoveredLine and hoveredLine.line[1] or "nothing"
	end
	c.plainText = new("TextListControl"):TextListControl(nil, {380, 40, 300, 120}, nil, { { height = 16, "^7Default column" }, { height = 20, "^xFFAA00Bigger line" }, { height = 12, "^8small" } })
	c.status = new("LabelControl"):LabelControl(nil, {20, 250, 0, 16}, function()
		local h = c.text.hoveredLine
		return string.format("^7clicked %s | hover %s %s | offset %d", self.clicked or "-", h and h.line[1] or "-", h and h.y or "-", c.text.controls.scrollBar.offset)
	end)
	c.path = new("PathControl"):PathControl(nil, {20, 290, 600, 24}, "C:/Users/me/Builds/", "Alpha/Beta/Gamma/", function(subPath)
		self.lastPath = subPath
	end)
	c.noBase = new("PathControl"):PathControl(nil, {20, 330, 600, 24}, "nobase", nil)
	c.undo = new("ButtonControl"):ButtonControl(nil, {20, 370, 80, 20}, "Undo", function()
		c.path:Undo()
	end)
	c.redo = new("ButtonControl"):ButtonControl(nil, {110, 370, 80, 20}, "Redo", function()
		c.path:Redo()
	end)
	c.deeper = new("ButtonControl"):ButtonControl(nil, {200, 370, 80, 20}, "Deeper", function()
		c.path:SetSubPath(c.path.subPath .. "Deep/")
	end)
	c.short = new("ButtonControl"):ButtonControl(nil, {290, 370, 80, 20}, "Short", function()
		c.path:SetSubPath("Alpha/")
	end)
	c.status2 = new("LabelControl"):LabelControl(nil, {20, 400, 0, 16}, function()
		return string.format("^7path '%s' onChange '%s' undo %d redo %d", c.path.subPath, self.lastPath or "-", #c.path.undo, #c.path.redo)
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
