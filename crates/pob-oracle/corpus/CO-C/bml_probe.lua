-- CO-C probe (crates/pob-oracle/tests/CO-C.rs): ConfigTab:BuildModList and the Data/ModMap.lua
-- applies under PoB.
--
-- Defines the global `__coc` with:
--   __coc.modmap()       -> JSON { "cases": [ { "key", "type", "tier", "roll", "effect", "ok", "err",
--                            "mods", "enemy" } ] }: every `data.mapMods.AffixData[k].apply` called
--                            with its call shape (ConfigOptions.lua:121-127) on a fresh ModList pair.
--   __coc.baseline()     -> JSON state of the current build (taken on the empty build after
--                            `reset()`); later cases send their state as a diff against it.
--   __coc.case(name)     -> JSON case for the build as loaded: pre-state (diff vs baseline), then
--                            `configTab:BuildModList()` under pcall, the resulting
--                            `configTab.modList` / `enemyModList` and the post-state (diff vs pre).
--   __coc.synthetic()    -> JSON { "cases": [...] } of the all-vars synthetic builds (see below),
--                            each a `case` taken after setting the config set and calling
--                            `configTab:UpdateControls()` (what Load does, so the controls agree
--                            with the input).
-- Mods use the dump shape (`$seq` tags) as `{"mod": {...}, "marks": {...}}`.

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
	if ml then
		for i, m in ipairs(ml) do out[i] = encMod(m) end
	end
	return "[" .. concat(out, ",") .. "]"
end

local function obj(map)
	local keys = { }
	for k in pairs(map) do keys[#keys + 1] = k end
	sort(keys)
	local parts = { }
	for _, k in ipairs(keys) do parts[#parts + 1] = jstr(k) .. ":" .. map[k] end
	return "{" .. concat(parts, ",") .. "}"
end

local opts = LoadModule("Modules/ConfigOptions")
local CONTROL_FIELDS = { "_className", "selIndex", "placeholder", "state", "enabled", "buf" }

__coc = { }

-- The state BuildModList reads or the applies write, as maps of encoded values.
local function state()
	local ct = build.configTab
	local set = ct.configSets[ct.activeConfigSetId]
	local st = { input = { }, placeholder = { }, controls = { } }
	for k, v in pairs(set.input) do st.input[k] = enc(v) end
	for k, v in pairs(set.placeholder) do st.placeholder[k] = enc(v) end
	for var, c in pairs(ct.varControls) do
		for _, f in ipairs(CONTROL_FIELDS) do
			local v = c[f]
			if type(v) ~= "function" then
				st.controls[var .. "|" .. f] = enc(v)
			end
		end
	end
	st.customModsList = enc(set.customModsList)
	st.enemyLevel = enc(ct.enemyLevel)
	st.characterLevel = enc(build.characterLevel)
	st.buildFlag = enc(build.buildFlag)
	return st
end

local function diff(base, cur)
	local out = { }
	for k, v in pairs(cur) do
		if base[k] ~= v then out[k] = v end
	end
	for k in pairs(base) do
		if cur[k] == nil then out[k] = "null" end
	end
	return out
end

local function encState(st, base)
	return obj({
		input = obj(base and diff(base.input, st.input) or st.input),
		placeholder = obj(base and diff(base.placeholder, st.placeholder) or st.placeholder),
		controls = obj(base and diff(base.controls, st.controls) or st.controls),
		customModsList = st.customModsList,
		enemyLevel = st.enemyLevel,
		characterLevel = st.characterLevel,
		buildFlag = st.buildFlag,
	})
end

-- Reset the active config set to NewConfigSet's defaults (ConfigTab.lua:1281-1297), keeping the
-- tables (`ct.input` aliases them).
local function reset()
	local ct = build.configTab
	local set = ct.configSets[ct.activeConfigSetId]
	wipeTable(set.input)
	wipeTable(set.placeholder)
	for _, varData in ipairs(opts) do
		if varData.var then
			set.input[varData.var] = varData.defaultState
			set.placeholder[varData.var] = varData.defaultPlaceholderState
			if varData.defaultIndex then
				set.input[varData.var] = varData.list[varData.defaultIndex].val
			end
		end
	end
	set.customModsList = { { title = "Default", enabled = true, text = "" } }
	build.buildFlag = false
end

local baseline
function __coc.baseline()
	reset()
	build.configTab:UpdateControls()
	baseline = state()
	return encState(baseline, nil)
end

function __coc.case(name)
	local ct = build.configTab
	local pre = state()
	local ok, err = pcall(ct.BuildModList, ct)
	local post = state()
	return obj({
		name = jstr(name),
		pre = encState(pre, baseline),
		ok = tostring(ok),
		err = ok and "null" or jstr(tostring(err)),
		mods = encList(ct.modList),
		enemy = encList(ct.enemyModList),
		post = encState(post, pre),
	})
end

-- Synthetic builds. Each `setup` writes into the reset config set.
local NUMS = { 1, 3, 250, 2.5, -2 }
local function optName(i, opt) return fmt("opt%d:%s", i, tostring(opt.var)) end
function __coc.synthetic()
	local ct = build.configTab
	local cases = { }
	local function run(name, setup)
		reset()
		local set = ct.configSets[ct.activeConfigSetId]
		setup(set)
		ct:UpdateControls()
		cases[#cases + 1] = __coc.case(name)
	end
	-- (1) one option at a time, every non-default value of its type
	for i, opt in ipairs(opts) do
		if opt.var and opt.apply then
			local t = opt.type
			if t == "check" then
				run(optName(i, opt) .. "=true", function(set) set.input[opt.var] = true end)
			elseif t == "list" then
				for j, e in ipairs(opt.list) do
					if j ~= (opt.defaultIndex or 1) or opt.defaultState ~= nil then
						run(optName(i, opt) .. "=" .. tostring(e.val), function(set) set.input[opt.var] = e.val end)
					end
				end
			elseif t == "text" then
				run(optName(i, opt) .. "=abc", function(set) set.input[opt.var] = "abc" end)
			else
				for _, v in ipairs(NUMS) do
					run(optName(i, opt) .. "=" .. tostring(v), function(set) set.input[opt.var] = v end)
				end
				-- the placeholder branch (input nil / input 0)
				run(optName(i, opt) .. " placeholder=7", function(set)
					set.input[opt.var] = nil
					set.placeholder[opt.var] = 7
				end)
				run(optName(i, opt) .. "=0 placeholder=4", function(set)
					set.input[opt.var] = 0
					set.placeholder[opt.var] = 4
				end)
				run(optName(i, opt) .. "=0", function(set) set.input[opt.var] = 0 end)
			end
		end
	end
	-- (2) all vars at once, in several variants
	local function allVars(variant)
		return function(set)
			for i, opt in ipairs(opts) do
				if opt.var and opt.apply then
					local t = opt.type
					if t == "check" then
						if variant ~= 3 or i % 2 == 0 then set.input[opt.var] = true end
					elseif t == "list" then
						local n = #opt.list
						local j = (variant == 1 and 2) or (variant == 2 and n) or ((i + variant) % n + 1)
						if opt.list[j] then set.input[opt.var] = opt.list[j].val end
					elseif t == "text" then
						set.input[opt.var] = "abc"
					else
						if variant == 4 then
							set.input[opt.var] = nil
							set.placeholder[opt.var] = (i % 9) + 1
						else
							set.input[opt.var] = NUMS[(i + variant) % #NUMS + 1]
						end
					end
				end
			end
		end
	end
	for variant = 1, 6 do
		run("allVars" .. variant, allVars(variant))
	end
	-- (3) every map affix in the prefix/suffix lists × every tier × two map mod effects
	local tierOpt, prefixOpt, suffixOpt
	for _, opt in ipairs(opts) do
		if opt.var == "multiplierMapModTier" then tierOpt = opt end
		if opt.var == "MapPrefix1" then prefixOpt = opt end
		if opt.var == "MapSuffix1" then suffixOpt = opt end
	end
	for _, tier in ipairs(tierOpt.list) do
		for _, effect in ipairs({ 0, 30 }) do
			for _, pair in ipairs({ { "MapPrefix1", prefixOpt }, { "MapSuffix2", suffixOpt } }) do
				for _, e in ipairs(pair[2].list) do
					run(fmt("map %s=%s tier=%s effect=%d", pair[1], e.val, tier.val, effect), function(set)
						set.input.multiplierMapModTier = tier.val
						set.input.multiplierMapModEffect = effect
						set.input[pair[1]] = e.val
					end)
				end
			end
		end
	end
	-- (4) custom modifier blocks and the `input.customMods` fallback
	local customCases = {
		{ "custom one block", { { title = "Default", enabled = true, text = "10% increased Damage\n+20 to maximum Life" } } },
		{ "custom escapes and blanks", { { title = "Default", enabled = true, text = "^7  15% increased Attack Speed  \n\n\t+1 to Level of all Fire Spell Skill Gems\r\nnot a real mod line\n" } } },
		{ "custom two blocks, one disabled", {
			{ title = "Offence", enabled = true, text = "20% more Damage\nAdds 10 to 20 Fire Damage" },
			{ title = "Off", enabled = false, text = "+50% to Fire Resistance" },
			{ title = 5, enabled = true, text = "You have Onslaught" },
		} },
		{ "custom untitled block", { { enabled = true, text = "Your Hits can't be Evaded" } } },
		{ "custom empty blocks", { { title = "Default", enabled = true, text = "" }, { title = "B", text = "" } } },
		{ "custom nil enabled", { { title = "B", text = "+1 to Maximum Power Charges\nGain 10% of Physical Damage as Extra Fire Damage" } } },
		{ "custom multi-mod line", { { title = "M", enabled = true, text = "+10% to all Elemental Resistances\n+5 to all Attributes" } } },
	}
	for _, c in ipairs(customCases) do
		run(c[1], function(set) set.customModsList = c[2] end)
	end
	run("custom fallback input.customMods", function(set)
		set.customModsList = { { title = "Default", enabled = true, text = "" } }
		set.input.customMods = "10% increased Damage\n  ^xFF0000+30 to Strength\nunparsed nonsense"
	end)
	run("custom fallback with block text", function(set)
		set.customModsList = { { title = "Default", enabled = true, text = "5% increased Damage" } }
		set.input.customMods = "+30 to Strength"
	end)
	run("custom fallback no list", function(set)
		set.customModsList = nil
		set.input.customMods = "+40 to Dexterity\n"
	end)
	return '{"cases":[' .. concat(cases, ",\n") .. "]}"
end

-- Data/ModMap.lua applies, called directly with each call shape.
function __coc.modmap()
	local cases = { }
	local keys = { }
	for k in pairs(data.mapMods.AffixData) do keys[#keys + 1] = k end
	sort(keys)
	for _, k in ipairs(keys) do
		local a = data.mapMods.AffixData[k]
		if a.apply then
			local function rec(tier, roll, effect, f)
				local ml, eml = new("ModList"):ModList(), new("ModList"):ModList()
				local ok, err = pcall(f, ml, eml)
				cases[#cases + 1] = obj({
					key = jstr(k), type = enc(a.type), tier = enc(tier), roll = enc(roll), effect = enc(effect),
					ok = tostring(ok), err = ok and "null" or jstr(tostring(err)),
					mods = encList(ml), enemy = encList(eml),
				})
			end
			for _, effect in ipairs({ 1, 1.3, 0.75 }) do
				if a.type == "check" then
					rec(nil, nil, effect, function(ml, eml) a.apply(k, effect, ml, eml) end)
				elseif a.type == "list" then
					for _, tier in ipairs({ 0, 1, 2, 3, 4, 1.5 }) do
						rec(tier, nil, effect, function(ml, eml) a.apply(tier, effect, a.values, ml, eml) end)
					end
				elseif a.type == "count" then
					for _, tier in ipairs({ 0, 1, 2, 3, 4 }) do
						for _, roll in ipairs({ 0, 37, 100 }) do
							rec(tier, roll, effect, function(ml, eml) a.apply(tier, roll, effect, a.values, ml, eml) end)
						end
					end
				end
			end
		end
	end
	return '{"cases":[' .. concat(cases, ",\n") .. "]}"
end
