-- Gallery "u11_single" (U11): single-line EditControls. The default filter with a prompt and a
-- tooltip; "%D" and "^%-%d" numeric edits (+/- buttons, wheel, UP/DOWN, numberInc, placeholder,
-- limit); a "%c" clearable edit with a limit; protected (FIXED) mode; inactiveText as a function
-- and as a string; a placeholder with enterFunc and borderFunc; a pasteFilter; a disabled edit;
-- an URL edit (Ctrl+click opens it); a tab group. Typing, selection by drag, shift+arrows and
-- double click, Ctrl+A/C/V/X/Z/Y, ctrl word moves and deletes, HOME/END, ESCAPE.
-- A status label prints the caret byte indices. Rust counterpart: src/gallery/u11_single.rs.
-- Interactions: u11_single.pobscript.

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
	self.enters = 0
	self.last = ""
	local function changed(buf)
		self.changes = self.changes + 1
		self.last = buf
	end
	c.plain = new("EditControl"):EditControl(nil, {20, 20, 260, 20}, "Hello there", "Name", nil, nil, changed)
	c.plain.tooltipText = "^7Type a name.\nOnly letters, digits and punctuation."
	c.num = new("EditControl"):EditControl(nil, {20, 50, 120, 20}, "5", nil, "%D", 4, changed)
	c.num:AddToTabGroup(c.plain)
	c.signed = new("EditControl"):EditControl(nil, {160, 50, 120, 20}, "", nil, "^%-%d", nil, changed)
	c.signed:SetPlaceholder("3")
	c.signed.numberInc = 2
	c.signed:AddToTabGroup(c.plain)
	c.clear = new("EditControl"):EditControl(nil, {20, 80, 200, 20}, "clear me", nil, "%c", 12, changed, nil, nil, true)
	c.secret = new("EditControl"):EditControl(nil, {20, 110, 200, 20}, "hunter2", "Pass", nil, nil, changed)
	c.secret:SetProtected(true)
	c.inactive = new("EditControl"):EditControl(nil, {20, 140, 200, 20}, "raw", nil, nil, nil, changed)
	c.inactive.inactiveText = function(buf)
		return "^xFF8000[" .. buf .. "] " .. #buf
	end
	c.inactiveStr = new("EditControl"):EditControl(nil, {240, 140, 200, 20}, "hidden", nil, nil, nil, changed)
	c.inactiveStr.inactiveText = "^1fixed text"
	c.inactiveStr.inactiveCol = "^3"
	c.ph = new("EditControl"):EditControl(nil, {20, 170, 200, 20}, "", nil, nil, nil, changed)
	c.ph:SetPlaceholder("Search...", true)
	c.ph.enterFunc = function(buf)
		self.enters = self.enters + 1
		self.last = "enter:" .. buf
	end
	c.ph.borderFunc = function()
		return 0.8, 0.2, 0.2
	end
	c.paste = new("EditControl"):EditControl(nil, {20, 200, 300, 20}, "", "Paste", "%c", nil, changed)
	c.paste.pasteFilter = function(text)
		return text:upper()
	end
	c.disabled = new("EditControl"):EditControl(nil, {320, 20, 150, 20}, "nope", "Off", nil, nil, changed) {
		enabled = false,
	}
	c.url = new("EditControl"):EditControl(nil, {340, 200, 300, 20}, "see https://example.com/x", nil, nil, nil, changed)
	c.status = new("LabelControl"):LabelControl(nil, {20, 240, 0, 16}, function()
		return string.format("^7caret %d sel %s changes %d enters %d last '%s' num '%s' signed '%s'",
			c.plain.caret, tostring(c.plain.sel), self.changes, self.enters, self.last, c.num.buf, c.signed.buf)
	end)
	c.status2 = new("LabelControl"):LabelControl(nil, {20, 260, 0, 16}, function()
		return string.format("^7paste '%s' clear '%s' secret '%s' undo %d redo %d",
			c.paste.buf, c.clear.buf, c.secret.buf, #c.plain.undo, #c.plain.redo)
	end)
end

function G:OnFrame(inputEvents, viewPort)
	self.host:ProcessControlsInput(inputEvents, viewPort)
	SetDrawColor(0.1, 0.1, 0.1)
	DrawImage(nil, viewPort.x, viewPort.y, viewPort.width, viewPort.height)
	self.host:DrawControls(viewPort)
end

return G
