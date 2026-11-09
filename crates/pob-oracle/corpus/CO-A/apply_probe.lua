-- CO-A apply probe (crates/pob-oracle/tests/CO-A.rs).
--
-- Runs after a build has loaded (`build` = the BUILD mode) and `__coa_want = { [i] = true }`
-- names the ConfigOptions indices to probe. For every probed option it calls the option's
-- `apply(val, modList, enemyModList, build)` (ConfigTab:BuildModList's call, ConfigTab.lua:1131-
-- 1160) on a fresh `new("ModList"):ModList()` pair, once per value of `values(opt)`: the option's default plus
-- non-default values of its type (and a few the UI never sends, to pin the dynamic `val`
-- semantics: wrong types, numeric strings). Each call is `pcall`ed. Returns JSON:
--   { "cases": [ { "i", "var", "type", "val", "ok", "err", "mods": [...], "enemy": [...],
--                  "radiusPlaceholder" } ] }
-- Mods use the dump shape (`$seq` tags) as `{"mod": {...}, "marks": {...}}`.
--
-- Fixed control state (mapAffixDropDownFunction reads it): input.multiplierMapModEffect = 30,
-- varControls.multiplierMapModTier.selIndex = 2. varControls.enemyRadius.placeholder is reset
-- to "<unset>" before each call (enemySizePreset writes it).

local fmt = string.format
local concat = table.concat
local sort = table.sort
local floor = math.floor
local huge = math.huge

local escapes = { ['"'] = '\\"', ['\\'] = '\\\\', ['\n'] = '\\n', ['\t'] = '\\t', ['\r'] = '\\r' }
local function jstr(s)
	return '"' .. s:gsub('[%c"\\]', function(c)
		return escapes[c] or fmt("\\u%04x", c:byte())
	end) .. '"'
end
local function jnum(v)
	if v ~= v then return '{"$float":"nan"}' end
	if v == huge then return '{"$float":"inf"}' end
	if v == -huge then return '{"$float":"-inf"}' end
	if v == 0 and 1 / v < 0 then return '{"$float":"-0"}' end
	if v == floor(v) and v > -2^53 and v < 2^53 then return fmt("%d", v) end
	for _, p in ipairs({ "%.15g", "%.16g" }) do
		local s = fmt(p, v)
		if tonumber(s) == v then return s end
	end
	return fmt("%.17g", v)
end
local function numKey(k)
	return (jnum(k):gsub('^{"%$float":"(.*)"}$', "%1"))
end

local enc
local function encTable(t, depth, seen)
	local n = 0
	while rawget(t, n + 1) ~= nil do n = n + 1 end
	local strKeys, numKeys, other = { }, { }, false
	for k in pairs(t) do
		if type(k) == "number" and k == floor(k) and k >= 1 and k <= n then
		elseif type(k) == "string" then strKeys[#strKeys + 1] = k
		elseif type(k) == "number" then numKeys[#numKeys + 1] = k
		else other = true end
	end
	if n >= 1 and #strKeys == 0 and #numKeys == 0 and not other then
		local parts = { }
		for i = 1, n do parts[i] = enc(t[i], depth + 1, seen) end
		return "[" .. concat(parts, ",") .. "]"
	end
	sort(strKeys)
	sort(numKeys)
	local parts = { }
	for _, k in ipairs(strKeys) do
		local jk = k:sub(1, 1) == "$" and "$" .. k or k
		parts[#parts + 1] = jstr(jk) .. ":" .. enc(t[k], depth + 1, seen)
	end
	if n >= 1 then
		local seq = { }
		for i = 1, n do seq[i] = enc(t[i], depth + 1, seen) end
		parts[#parts + 1] = '"$seq":[' .. concat(seq, ",") .. "]"
	end
	if #numKeys > 0 then
		local np = { }
		for _, k in ipairs(numKeys) do np[#np + 1] = jstr(numKey(k)) .. ":" .. enc(t[k], depth + 1, seen) end
		parts[#parts + 1] = '"$num":{' .. concat(np, ",") .. "}"
	end
	return "{" .. concat(parts, ",") .. "}"
end
enc = function(v, depth, seen)
	depth = depth or 0
	seen = seen or { }
	local tv = type(v)
	if v == nil then return "null"
	elseif tv == "boolean" then return v and "true" or "false"
	elseif tv == "number" then return jnum(v)
	elseif tv == "string" then return jstr(v)
	elseif tv == "table" then
		if getmetatable(v) and rawget(v, "Object") then
			return jstr("<object " .. tostring(v._className) .. ">")
		end
		if seen[v] or depth > 12 then return jstr("<cycle>") end
		seen[v] = true
		local s = encTable(v, depth, seen)
		seen[v] = nil
		return s
	end
	return jstr("<" .. tv .. ">")
end

local stdKey = { name = true, type = true, value = true, flags = true, keywordFlags = true, source = true }
local function encMod(m)
	local n = 0
	while rawget(m, n + 1) ~= nil do n = n + 1 end
	local parts, marks, keys, nums = { }, { }, { }, { }
	for k in pairs(m) do keys[#keys + 1] = k end
	sort(keys, function(a, b) return tostring(a) < tostring(b) end)
	for _, k in ipairs(keys) do
		if type(k) == "string" and stdKey[k] then
			parts[#parts + 1] = jstr(k) .. ":" .. enc(m[k])
		elseif type(k) == "string" then
			marks[#marks + 1] = jstr(k) .. ":" .. enc(m[k])
		elseif type(k) == "number" and (k > n or k < 1 or k ~= floor(k)) then
			nums[#nums + 1] = jstr(numKey(k)) .. ":" .. enc(m[k])
		end
	end
	if n >= 1 then
		local seq = { }
		for i = 1, n do seq[i] = enc(m[i]) end
		parts[#parts + 1] = '"$seq":[' .. concat(seq, ",") .. "]"
	end
	if #nums > 0 then parts[#parts + 1] = '"$num":{' .. concat(nums, ",") .. "}" end
	return '{"mod":{' .. concat(parts, ",") .. '},"marks":{' .. concat(marks, ",") .. "}}"
end

local function encList(ml)
	local out = { }
	for i, m in ipairs(ml) do out[i] = encMod(m) end
	return "[" .. concat(out, ",") .. "]"
end

-- The probed values: { n = count, [1..n] = value } (a list `val` may be nil).
local NUMS = { 1, 3, 250, -2, 2.5, 0, "7" }
local function values(opt)
	local out = { n = 0 }
	local function add(v) out.n = out.n + 1; out[out.n] = v end
	local t = opt.type
	if t == "check" then
		-- BuildModList passes `true`; the rest pin the closures' handling of `val`
		add(true); add(false); add(1); add("x")
	elseif t == "list" then
		local d = opt.list[opt.defaultIndex or 1]
		if opt.defaultState ~= nil then
			add(opt.defaultState)
		elseif d then
			add(d.val)
		end
		for _, e in ipairs(opt.list) do
			if e ~= d then add(e.val) end
		end
		add("<not a list value>"); add(1)
	elseif t == "text" then
		add(opt.defaultState or ""); add("abc"); add("1"); add("line one\nline two")
	else
		-- count / integer / countAllowZero / float
		add(opt.defaultState or opt.defaultPlaceholderState or 0)
		for _, v in ipairs(NUMS) do add(v) end
		add(true)
	end
	return out
end

local opts = LoadModule("Modules/ConfigOptions")
local ct = build.configTab
ct.input.multiplierMapModEffect = 30
ct.varControls.multiplierMapModTier.selIndex = 2
local cases = { }
for i, opt in ipairs(opts) do
	if __coa_want[i] and opt.apply then
		local vals = values(opt)
		for k = 1, vals.n do
			local v = vals[k]
			local ml, eml = new("ModList"):ModList(), new("ModList"):ModList()
			ct.varControls.enemyRadius.placeholder = "<unset>"
			local ok, err = pcall(opt.apply, v, ml, eml, build)
			cases[#cases + 1] = "{" .. concat({
				'"i":' .. i,
				'"var":' .. enc(opt.var),
				'"type":' .. enc(opt.type),
				'"val":' .. enc(v),
				'"ok":' .. tostring(ok),
				'"err":' .. (ok and "null" or jstr(tostring(err))),
				'"mods":' .. encList(ml),
				'"enemy":' .. encList(eml),
				'"radiusPlaceholder":' .. enc(ct.varControls.enemyRadius.placeholder),
			}, ",") .. "}"
		end
	end
end
return '{"cases":[' .. concat(cases, ",\n") .. "]}"
