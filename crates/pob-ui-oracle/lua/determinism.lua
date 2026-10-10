-- pob-ui-oracle: determinism patches 1 and 2 (ui doc §11.2). Loaded by boot.rs after the
-- SimpleGraphic globals are bound to Rust and before any PoB file runs.
--
-- 1. Virtual clock. `GetTime()` is the host's virtual clock (+16 ms per frame, Sys::time_ms).
--    `os.time()` without a table is a fixed epoch plus the virtual clock in whole seconds;
--    `os.clock()` is the virtual clock in seconds; `os.date(fmt[, t])` formats `t` (default: the
--    virtual `os.time()`) in UTC (a leading "!" is added), so neither the wall clock nor the
--    machine's time zone reaches a trace.
-- 2. `math.randomseed(0)` now, and every later `math.randomseed(x)` (PoEAPI.lua:94 seeds with
--    os.time()) reseeds with 0; `math.random` is wrapped (same LuaJIT generator).

local raw_time, raw_date = os.time, os.date
local m_floor = math.floor
local VIRTUAL_EPOCH = 1700000000

os.time = function(t)
	if t ~= nil then
		return raw_time(t)
	end
	return VIRTUAL_EPOCH + m_floor(GetTime() / 1000)
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
