-- U5: generates utf8_vectors.txt, the oracle vectors for pob_ui_core::luastr::utf8 and
-- luastr::format_num_sep (tests/luastr.rs). It runs the REAL lua-utf8 C module (luautf8
-- lutf8lib.c at bdd3d7f, the commit PathOfBuilding-SimpleGraphic 3b1a346 vendors and builds
-- as runtime/lua-utf8.dll) under LuaJIT, and PoB's own formatNumSep text (Common.lua:803-828).
--
-- Regenerate (any LuaJIT 2.1 with headers; lua-utf8 cannot be loaded into the vendored mlua
-- LuaJIT, so the vectors are a checked-in fixture):
--   git clone https://github.com/starwing/luautf8 && git -C luautf8 checkout bdd3d7f
--   gcc -O2 -shared -fPIC -include limits.h -I/usr/include/luajit-2.1 \
--       -o lua-utf8.so luautf8/lutf8lib.c
--   LUA_CPATH='./?.so' luajit crates/pob-ui-core/tests/luastr_data/gen_utf8_vectors.lua \
--       <PathOfBuilding>/src/Modules/Common.lua > crates/pob-ui-core/tests/luastr_data/utf8_vectors.txt
--
-- Only integer-valued (and a few fractional, in-range) number arguments are generated: luautf8
-- reads them with lua_tointeger, whose out-of-range/NaN result depends on the CPU (x64
-- cvttsd2si vs ARM64 fcvtzs); the shipped x64 behaviour is covered by tests/luastr.rs directly.
--
-- Line format: fn TAB arg... TAB "=>" TAB result...; a string is s:<bytes, \xHH-escaped outside
-- 0x21-0x7E and for '\'>, a number n:<%.17g>, nil is "nil", an error is "!err" TAB <message>.

local u = require("lua-utf8")
local commonPath = assert(arg[1], "usage: gen_utf8_vectors.lua <Modules/Common.lua>")

-- Park-Miller minimal standard generator (exact in doubles).
local seed = 20261010
local function rnd(n) -- 1..n
	seed = (seed * 16807) % 2147483647
	return seed % n + 1
end
local function pick(t) return t[rnd(#t)] end
local function chance(p) return rnd(1000) <= p * 1000 end

local function esc(s)
	local out = {}
	for i = 1, #s do
		local b = s:byte(i)
		if b >= 33 and b <= 126 and b ~= 92 then
			out[#out + 1] = string.char(b)
		else
			out[#out + 1] = string.format("\\x%02X", b)
		end
	end
	return table.concat(out)
end

local function val(v)
	if v == nil then return "nil" end
	if type(v) == "number" then return "n:" .. string.format("%.17g", v) end
	if type(v) == "boolean" then return v and "true" or "false" end
	return "s:" .. esc(v)
end

local lines = {}
local function emit(fn, args, ok, ...)
	local parts = { fn }
	for i = 1, args.n do parts[#parts + 1] = val(args[i]) end
	parts[#parts + 1] = "=>"
	if not ok then
		parts[#parts + 1] = "!err"
		parts[#parts + 1] = esc(tostring((...)))
	else
		local res = { n = select("#", ...), ... }
		for i = 1, res.n do parts[#parts + 1] = val(res[i]) end
	end
	lines[#lines + 1] = table.concat(parts, "\t")
end
local function run(fn, f, ...)
	local args = { n = select("#", ...), ... }
	emit(fn, args, pcall(f, ...))
end

-- Subject pieces: ASCII (letters, digits, space, punctuation, controls), 2/3/4-byte characters,
-- a combining mark (%t, reverse), Unicode digits/space, and invalid bytes (lone lead, lone
-- continuation, 0xFF).
local pieces = {
	"a", "b", "Z", "x", "0", "5", "9", " ", "\t", "\n", ".", ",", "-", "_", "%", "(", ")", "[", "]",
	"^", "$", "'", "a", "e", "1", "2", "3",
	"\195\169", "\195\182", "\195\132", "\226\130\172", "\228\184\173", "\240\159\152\128",
	"\204\129", "\194\160", "\227\128\128", "\217\163", "\239\188\144", "\206\169", "\195\159",
	"\208\150",
}
local badPieces = { "\195", "\128", "\255", "\226\130" }
local function subject(maxLen)
	local t = {}
	for _ = 1, rnd(maxLen + 1) - 1 do
		if chance(0.04) then t[#t + 1] = pick(badPieces) else t[#t + 1] = pick(pieces) end
	end
	return table.concat(t)
end

local items = {
	".", "%a", "%A", "%c", "%d", "%D", "%l", "%L", "%p", "%P", "%s", "%S", "%u", "%U", "%w", "%W",
	"%x", "%g", "%G", "%t", "%T", "%z", "a", "b", "e", "1", "\195\169", "\228\184\173", "%%", "%.",
	"%(", " ", "[%s%p]", "[^\n]", "[a-z\195\169]", "[\195\160-\195\182]", "[^%w]", "[%d.]",
	"[\195\169-\228\184\173]", "[%a_]", "[]a]", "[^]]", "[a-]", "\204\129", "\217\163",
}
local quants = { "", "", "", "*", "+", "-", "?" }
local function pattern()
	if chance(0.15) then
		return pick({ "[%s%p]*$", "%w*$", "^%w*", "^[%s%p]*", "(%d%d%d)", "%.", "()([^\n]*)\n()",
			"(-?)(%d+)(%.?%d*)", "%b()", "%f[%w]%w+", "(a)%1", "(%w+) (%w+)", "^(%a)", "()",
			"%", "[a", "(a", "a)", "%b", "%f", "%9", "%1", "(()", "%g+", "[%z]" })
	end
	local t = {}
	if chance(0.2) then t[#t + 1] = "^" end
	local open = 0
	for _ = 1, rnd(4) do
		local r = rnd(10)
		if r == 1 then
			t[#t + 1] = "("; open = open + 1
		elseif r == 2 and open > 0 then
			t[#t + 1] = ")"; open = open - 1
		elseif r == 3 then
			t[#t + 1] = "()"
		else
			t[#t + 1] = pick(items) .. pick(quants)
		end
	end
	for _ = 1, open do t[#t + 1] = ")" end
	if chance(0.15) then t[#t + 1] = "$" end
	return table.concat(t)
end

local inits = { 1, 2, 3, -1, -2, -3, 0, 5, 10, -10, 2.5, -1.5, 4294967298 }
local function optnum(t)
	local i = rnd(#t + 2)
	if i > #t then return nil end
	return t[i]
end

-- utf8.sub
for _ = 1, 700 do
	local s = subject(9)
	local i = pick({ -8, -5, -3, -2, -1, 0, 1, 2, 3, 4, 6, 9, 2.5, -2.5, 4294967297, -4294967295 })
	local j = optnum({ -8, -5, -3, -2, -1, 0, 1, 2, 3, 4, 6, 9, 2.5, 4294967298 })
	if j == nil then run("sub", u.sub, s, i) else run("sub", u.sub, s, i, j) end
end
-- utf8.reverse (strict and lax)
for _ = 1, 400 do
	local s = subject(8)
	if chance(0.5) then run("reverse", u.reverse, s) else run("reverse", u.reverse, s, true) end
end
-- utf8.next. Calls whose C reads memory outside the string are undefined (and differ run to
-- run): a start position past #s + 1 walked backwards or read in place, or position 0 read in
-- place (push_offset reads s[-1]). They are not generated (the port reads 0 there).
local function byteRelat(pos, len)
	if pos >= 0 then return pos elseif -pos > len then return 0 else return len + pos + 1 end
end
local nextCount = 0
while nextCount < 800 do
	local s = subject(7)
	local i = optnum({ -6, -3, -1, 0, 1, 2, 3, 4, 5, 7, 9, 12 })
	local off = optnum({ -3, -2, -1, 0, 1, 2, 3 })
	local offset = byteRelat(i or 1, #s)
	local idx = off or (i ~= nil and 1 or 0)
	if (offset > #s + 1 and idx <= 0) or (offset == 0 and idx == 0) then
		-- undefined in C: skip
	elseif i == nil and off == nil then
		nextCount = nextCount + 1
		run("next", u.next, s)
	elseif off == nil then
		nextCount = nextCount + 1
		run("next", u.next, s, i)
	else
		nextCount = nextCount + 1
		run("next", u.next, s, i, off)
	end
end
-- utf8.find / utf8.match
for _ = 1, 2500 do
	local s = subject(10)
	local p = chance(0.1) and subject(2) or pattern()
	local init = optnum(inits)
	if chance(0.5) then
		local plain = chance(0.3) and pick({ true, 2 }) or nil
		if plain ~= nil then
			run("find", u.find, s, p, init or 1, plain)
		elseif init ~= nil then
			run("find", u.find, s, p, init)
		else
			run("find", u.find, s, p)
		end
	else
		if init ~= nil then run("match", u.match, s, p, init) else run("match", u.match, s, p) end
	end
end
-- utf8.gsub (string replacements)
local repls = { "%1,", "<%0>", "x", "%%", "\195\169", "", "%1%2", "%a", "%", "%\217\163", "[%1]",
	"\195", "%0%0", "-" }
for _ = 1, 1200 do
	local s = subject(10)
	local p = pattern()
	local r = pick(repls)
	local n = optnum({ 0, 1, 2, -1, 3, 1.5 })
	if n ~= nil then run("gsub", u.gsub, s, p, r, n) else run("gsub", u.gsub, s, p, r) end
end

-- formatNumSep: PoB's own text (Common.lua:803-828) over the real module.
local src = {}
local ln = 0
for line in io.lines(commonPath) do
	ln = ln + 1
	if ln >= 803 and ln <= 828 then src[#src + 1] = line end
end
assert(src[1]:match("^function formatNumSep%(str%)"), "Common.lua:803 is not formatNumSep")
local env = setmetatable({}, { __index = _G })
local chunk = assert(loadstring("local utf8 = ...\n" .. table.concat(src, "\n") .. "\nreturn formatNumSep"))
setfenv(chunk, env)
local formatNumSep = chunk(u)

local numStrs = { "1234.56", "123", "1234567", "-1234567.891", "^71234", "^x33FF771234567",
	"^x12345678", "^12", "^x12", "12 and 345678 and ^4-9876.5", "0.5", ".5", "1e10", "100",
	"1000", "999999", "-0.25", "12.", "^xFFFFFF", "^1^2345", "Life: 4567 / ^x33FF77+12345.75%",
	"1.2.3.4", "x1234567y", "^x1G2345678" }
local seps = { ",", ".", " ", "", "\194\160", "'", "%", "\195\169", "ab", "e\204\129", "%%", "%1" }
local decs = { ".", ",", "%", "%%", "", "\195\169" }
for _ = 1, 600 do
	local s
	if chance(0.5) then
		s = pick(numStrs)
	else
		local t = {}
		for _ = 1, rnd(3) do
			t[#t + 1] = pick({ "", "^7", "^x", "^xAABBCC", "-", " ", "x" }) .. tostring(rnd(10 ^ rnd(9)))
				.. pick({ "", ".", "." .. rnd(999), " " })
		end
		s = table.concat(t, pick({ " ", ", ", "/" }))
	end
	local show = chance(0.75)
	local sep, dec = pick(seps), pick(decs)
	env.main = { showThousandsSeparators = show, thousandsSeparator = sep, decimalSeparator = dec }
	local args = { n = 4, s, show, sep, dec }
	emit("fns", args, pcall(formatNumSep, s))
end

io.write(table.concat(lines, "\n"), "\n")
