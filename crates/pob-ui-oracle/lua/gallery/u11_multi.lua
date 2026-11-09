-- Gallery "u11_multi" (U11): multi-line EditControls (lineHeight) and ResizableEditControl.
-- Typing through "^%C\t\n" (RETURN inserts "\n", the Tab char arrives via SG's extra OnChar),
-- UP/DOWN/PAGEUP/PAGEDOWN/HOME/END (plain and ctrl), multi-line selection by shift+arrows and
-- drag, double click word/run selection, Ctrl+C/X/V/Z/Y, ctrl backspace/delete across lines, both
-- scrollbars (content growth, wheel, a click on a scrollbar arrow through the edit's child
-- dispatch), zoom (Ctrl +/-/0 and Ctrl+wheel), and the resize dragger (drag, right click
-- min/max toggle). Rust counterpart: src/gallery/u11_multi.rs. Interactions: u11_multi.pobscript.

-- The oracle's `lua-utf8` stand-in (crates/pob-oracle/src/jit.rs PRELUDE) has no `utf8.next`,
-- which EditControl.lua:558/:572 call (CCR-0033 item 4). This is luautf8's Lutf8_next
-- (lutf8lib.c:1038-1043, push_offset/utf8_offset) for the call shape EditControl uses
-- (`utf8.next(s, i, off)` with i >= 1), installed into the module table EditControl's local
-- `utf8` is (require caches it).
local lutf8 = require("lua-utf8")
if not lutf8.next then
	local function iscont(s, p) -- p is a 0-based byte offset; past the end reads the NUL
		local b = s:byte(p + 1) or 0
		return b >= 0x80 and b < 0xC0
	end
	lutf8.next = function(s, i, off)
		local e = #s
		local p = i - 1
		local idx = off
		if idx >= 0 then
			while p < e and idx > 0 do
				while p < e and iscont(s, p + 1) do
					p = p + 1
				end
				p = (p < e) and p + 1 or e
				idx = idx - 1
			end
		else
			while 0 < p and idx < 0 do
				local q = p
				while 0 < q and iscont(s, q - 1) do
					q = q - 1
				end
				p = (0 < q) and q - 1 or 0
				idx = idx + 1
			end
		end
		if idx ~= 0 or p == e then
			return
		end
		return p + 1, s:byte(p + 1)
	end
end

local G = { }

function G:Init()
	self.host = new("ControlHost"):ControlHost()
	local c = self.host.controls
	self.changes = 0
	local function changed(buf)
		self.changes = self.changes + 1
	end
	c.notes = new("EditControl"):EditControl(nil, {20, 20, 400, 160},
		"Line one has words\nsecond line, with punctuation!!\n\tindented   spaces\nfour\n\nsix after a blank",
		nil, "^%C\t\n", nil, changed, 16, true)
	c.resize = new("ResizableEditControl"):ResizableEditControl(nil, {460, 20, 300, 100, 200, 60, 500, 300},
		"resize me\nline 2", nil, "^%C\t\n", nil, changed, 14)
	c.small = new("EditControl"):EditControl(nil, {20, 220, 240, 80},
		"a\nb\nc\nd\ne\nf\ng\nh a much longer line that does not fit in the box at all\ni\nj", nil, "^%C\t\n", nil, changed, 14)
	c.status = new("LabelControl"):LabelControl(nil, {20, 320, 0, 16}, function()
		local n = c.notes
		return string.format("^7caret %d sel %s lh %s offV %s offH %s changes %d undo %d",
			n.caret, tostring(n.sel), tostring(n.lineHeight), tostring(n.controls.scrollBarV.offset),
			tostring(n.controls.scrollBarH.offset), self.changes, #n.undo)
	end)
	c.status2 = new("LabelControl"):LabelControl(nil, {20, 340, 0, 16}, function()
		local s, r = c.small, c.resize
		return string.format("^7small caret %d offV %s offH %s resize %sx%s caret %d",
			s.caret, tostring(s.controls.scrollBarV.offset), tostring(s.controls.scrollBarH.offset),
			tostring(r.width), tostring(r.height), r.caret)
	end)
end

function G:OnFrame(inputEvents, viewPort)
	self.host:ProcessControlsInput(inputEvents, viewPort)
	SetDrawColor(0.1, 0.1, 0.1)
	DrawImage(nil, viewPort.x, viewPort.y, viewPort.width, viewPort.height)
	self.host:DrawControls(viewPort)
end

return G
