-- pob-ui-oracle: determinism patches 1 and 2 (ui doc §11.2). Loaded by boot.rs after the
-- SimpleGraphic globals are bound to Rust and before any PoB file runs.
--
-- 1. Virtual clock. `GetTime()` is the host's virtual clock (+16 ms per frame, Sys::time_ms).
--    `os.time()` without a table is a fixed epoch plus the virtual clock in whole seconds;
--    `os.time(t)` converts the date table as UTC (timegm, not the C library's local-time
--    mktime); `os.clock()` is the virtual clock in seconds; `os.date(fmt[, t])` formats `t`
--    (default: the virtual `os.time()`) in UTC (a leading "!" is added). So neither the wall
--    clock nor the machine's time zone reaches a trace.
-- 2. `math.randomseed(0)` now, and every later `math.randomseed(x)` (PoEAPI.lua:94 seeds with
--    os.time()) reseeds with 0; `math.random` is wrapped (same LuaJIT generator).
-- 3. Write times. `io.open(path, mode)` with "w" or "a" in `mode` notes the file
--    (`__oracleNoteWrite`), so `GetFileModifiedTime` reports the virtual write time for it and 0
--    for untouched fixture files (U7's headless rule, CCR-0028 item 10).

local raw_time, raw_date = os.time, os.date
local m_floor, m_ceil = math.floor, math.ceil
local type, tonumber, error = type, tonumber, error
local VIRTUAL_EPOCH = 1700000000

-- Days since 1970-01-01 of a proleptic Gregorian date (month 1..12).
local function days_from_civil(y, m, d)
	if m <= 2 then
		y = y - 1
	end
	local era = m_floor(y / 400)
	local yoe = y - era * 400
	local mp = (m + 9) % 12
	local doy = m_floor((153 * mp + 2) / 5) + d - 1
	local doe = yoe * 365 + m_floor(yoe / 4) - m_floor(yoe / 100) + doy
	return era * 146097 + doe - 719468
end

-- lib_os.c getfield: a number (or numeric string) truncated to an integer, else the default;
-- no default is LuaJIT's "field '%s' missing in date table".
local function getfield(t, key, d)
	local v = t[key]
	local n = (type(v) == "number" or type(v) == "string") and tonumber(v) or nil
	if n == nil then
		if d == nil then
			error("field '" .. key .. "' missing in date table", 3)
		end
		return d
	end
	return n >= 0 and m_floor(n) or m_ceil(n)
end

os.time = function(t)
	if t == nil then
		return VIRTUAL_EPOCH + m_floor(GetTime() / 1000)
	end
	if type(t) ~= "table" then
		return raw_time(t) -- the library's own argument error
	end
	-- lib_os.c reads the fields in this order (so the first missing one is reported).
	local sec = getfield(t, "sec", 0)
	local min = getfield(t, "min", 0)
	local hour = getfield(t, "hour", 12)
	local day = getfield(t, "day")
	local month = getfield(t, "month") - 1
	local year = getfield(t, "year")
	-- mktime normalises out-of-range fields; so does this (the month here, the rest linearly).
	year = year + m_floor(month / 12)
	month = month % 12
	return ((days_from_civil(year, month + 1, 1) + day - 1) * 24 + hour) * 3600 + min * 60 + sec
end

os.clock = function()
	return GetTime() / 1000
end

os.date = function(fmt, t)
	fmt = fmt or "%c"
	if fmt:sub(1, 1) ~= "!" then
		fmt = "!" .. fmt
	end
	return raw_date(fmt, t or os.time())
end

local raw_randomseed, raw_random = math.randomseed, math.random
raw_randomseed(0)
math.randomseed = function()
	raw_randomseed(0)
end
math.random = function(...)
	return raw_random(...)
end

local raw_open, note_write, abs = io.open, __oracleNoteWrite, __mk2_abs
io.open = function(path, mode, ...)
	local f, err, code = raw_open(path, mode, ...)
	if f and type(path) == "string" and type(mode) == "string" and mode:find("[wa]") then
		note_write(abs and abs(path) or path)
	end
	return f, err, code
end
