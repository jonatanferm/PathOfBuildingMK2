#@ SimpleGraphic
-- U2 Wine probe (ui doc §11.1 T6): measures text with the REAL SimpleGraphic.dll that ships with
-- PoB (`SimpleGraphic v2.5-3b1a346`) and writes the results as JSON for
-- `crates/pob-sg/tests/font_probe.rs`. See README.md in this directory for how to run it.
--
-- Output (relative to this script's directory):
--   out/metrics.json  DrawStringWidth / DrawStringCursorIndex for every case of cases.lua
--                     (schema: metrics.schema.json)
--   out/scene.json    the draw calls of the screenshot scene (for the pixel comparison, U53)
--   <runtime>/Screenshots/*.png  the scene, taken on frame 5 (TakeScreenshot, png)

local scriptPath = GetScriptPath()
local cases = dofile(scriptPath .. "/cases.lua")

local launch = { }
SetMainObject(launch)

local function jsonString(s)
	-- JSON string literal for an arbitrary Lua byte string (bytes >= 0x80 pass through; the
	-- probe's own strings are valid UTF-8).
	return '"' .. s:gsub('[%c"\\]', function(c)
		return string.format("\\u%04x", c:byte())
	end) .. '"'
end

local function jsonValue(v)
	local t = type(v)
	if t == "table" then
		local parts = { }
		if #v > 0 or next(v) == nil then
			for i = 1, #v do
				parts[i] = jsonValue(v[i])
			end
			return "[" .. table.concat(parts, ",") .. "]"
		end
		local keys = { }
		for k in pairs(v) do
			keys[#keys + 1] = k
		end
		table.sort(keys)
		for i, k in ipairs(keys) do
			parts[i] = jsonString(k) .. ":" .. jsonValue(v[k])
		end
		return "{" .. table.concat(parts, ",") .. "}"
	elseif t == "string" then
		return jsonString(v)
	elseif t == "boolean" then
		return tostring(v)
	else
		return string.format("%.17g", v)
	end
end

local function writeJson(name, value)
	MakeDir(scriptPath .. "/out")
	local f = assert(io.open(scriptPath .. "/out/" .. name, "wb"))
	f:write(jsonValue(value), "\n")
	f:close()
end

local function measure()
	-- Metrics are compared in physical pixels: force a DPI scale of 1.
	SetDPIScaleOverridePercent(100)
	local width, cursor = { }, { }
	for fi, font in ipairs(cases.fonts) do
		width[fi], cursor[fi] = { }, { }
		for hi, height in ipairs(cases.heights) do
			width[fi][hi], cursor[fi][hi] = { }, { }
			for si, text in ipairs(cases.strings) do
				width[fi][hi][si] = DrawStringWidth(height, font, text)
				local row = { }
				for pi, p in ipairs(cases.points) do
					row[pi] = DrawStringCursorIndex(height, font, text, p[1], p[2] * height + p[3])
				end
				cursor[fi][hi][si] = row
			end
		end
	end
	local screenW, screenH = GetScreenSize()
	writeJson("metrics.json", {
		schema = "pob-ui-oracle/probe/metrics.schema.json#1",
		screenScale = GetScreenScale(),
		dpiScaleOverridePercent = GetDPIScaleOverridePercent(),
		screenSize = { screenW, screenH },
		luaVersion = jit and jit.version or _VERSION,
		fonts = cases.fonts,
		heights = cases.heights,
		points = cases.points,
		strings = cases.strings,
		width = width,
		cursor = cursor,
	})
end

-- The screenshot scene: every call is recorded in out/scene.json so it can be replayed through
-- pob-sg. Colours leak from escapes into the following DrawImage (ui doc §2 "Escape leak").
-- "DrawImage" entries omit the image argument, which is always nil (the white image).
local scene = {
	{ "SetDrawColor", 0.1, 0.1, 0.1 },
	{ "DrawImage", 0, 0, 1280, 800 },
	{ "SetDrawColor", 1, 1, 1 },
	{ "DrawString", 10, 10, "LEFT", 16, "VAR", "LEFT ^1red ^xFFC040amber^7 white\ttab" },
	{ "DrawString", 0, 34, "CENTER", 20, "VAR BOLD", "CENTER ^2green" },
	{ "DrawString", 0, 60, "RIGHT", 14, "FIXED", "RIGHT fixed 0123456789" },
	{ "DrawString", 640, 84, "CENTER_X", 24, "FONTIN SC", "Center_X Fontin SmallCaps" },
	{ "DrawString", 1270, 116, "RIGHT_X", 18, "FONTIN ITALIC", "Right_X Fontin Italic" },
	{ "DrawString", 10, 140, "LEFT", 16, "VAR", "Tofu: caf\195\169 \230\151\165\230\156\172 \240\159\152\128" },
	{ "DrawString", 10, 164, "LEFT", 30, "VAR", "Height 30 (scaled 28 atlas)" },
	{ "DrawString", 10, 200, "LEFT", 13, "VAR", "Height 13 rounds to 14" },
	{ "DrawString", 10, 220, "LEFT", 16, "VAR", "Line one\nLine ^4two\n\tLine three" },
	{ "DrawString", 10, 280, "LEFT", 12, "FONTIN", "^x40FF40leaks into the next image" },
	{ "DrawImage", 10, 296, 200, 8 },
	{ "SetDrawColor", 1, 1, 1 },
	{ "DrawString", -30, 320, "LEFT", 16, "VAR", "Clipped at the left edge" },
	{ "DrawString", 10, -10, "LEFT", 16, "VAR", "^1off screen top" },
	{ "DrawImage", 300, 296, 200, 8 },
	{ "SetDrawColor", 1, 1, 1 },
	{ "DrawString", 10, 340, "LEFT", 64, "FONTIN SC ITALIC", "Sixty-four" },
	{ "DrawString", 10, 420, "LEFT", 100, "VAR", "Huge" },
}

function launch:OnInit()
	RenderInit("DPI_AWARE")
	ConExecute("set r_screenshotFormat png")
	measure()
	local screenW, screenH = GetScreenSize()
	writeJson("scene.json", { screenSize = { screenW, screenH }, screenScale = GetScreenScale(), calls = scene })
	self.frame = 0
end

function launch:OnFrame()
	self.frame = self.frame + 1
	SetDrawLayer(0)
	SetViewport()
	for _, call in ipairs(scene) do
		if call[1] == "DrawImage" then
			-- DrawImage(nil, left, top, width, height): nil binds the white image
			DrawImage(nil, unpack(call, 2))
		else
			_G[call[1]](unpack(call, 2))
		end
	end
	if self.frame == 5 then
		TakeScreenshot()
	elseif self.frame == 8 then
		Exit()
	end
end

function launch:OnKeyDown(key)
	if key == "ESCAPE" then
		Exit()
	end
end
