-- MP7 acceptance (crates/pob-oracle/tests/MP7.rs): PoB's side of the per-func jewel oracle
-- (calc doc §9.4 L2 "Jewel funcs: each func on every node of the 3_26 tree").
--
-- Input: `__mp7_lines` (array of mod lines). Each line is parsed with modLib.parseMod; the
-- first mod's `value.func` (a JewelFunc / ExtraJewelFunc closure) is run, as CalcSetup runs
-- radius funcs (CalcSetup.lua:146-151/195-197/293), on every node of the 3_26 tree (sorted by
-- id) plus synthetic tattoo / conquered overlays of every 7th / 11th node with stats:
--   out = new ModList holding the node's mods (keystoneMod for keystones, CalcSetup.lua:138-142)
--   func(node, out, data)   (pcall), one `data` per func, data.modSource = "Tree:61419"
-- then the finish call func(nil, out, data) on a fresh empty ModList.
-- Records: per node, the out list when it changed ({"$same": j} for an untouched mod j) and
-- the data table when it changed (ModLists in data as their length); the finish out and the
-- full data. Returns one JSON string.
local fmt, concat, sort = string.format, table.concat, table.sort
local floor, huge = math.floor, math.huge

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

-- A generic Lua value in the dump shape (pob_gamedata::Value deserialises it).
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
	elseif tv == "function" then return jstr("<function>")
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

-- A mod: the standard keys and `$seq` tags (`mod`), any other key in `marks`.
local stdKey = { name = true, type = true, value = true, flags = true, keywordFlags = true, source = true }
local function encMod(m)
	local n = 0
	while rawget(m, n + 1) ~= nil do n = n + 1 end
	local parts, marks = { }, { }
	local keys = { }
	for k in pairs(m) do keys[#keys + 1] = k end
	sort(keys, function(a, b) return tostring(a) < tostring(b) end)
	for _, k in ipairs(keys) do
		if type(k) == "string" and stdKey[k] then
			parts[#parts + 1] = jstr(k) .. ":" .. enc(m[k])
		elseif type(k) == "string" then
			marks[#marks + 1] = jstr(k) .. ":" .. enc(m[k])
		end
	end
	if n >= 1 then
		local seq = { }
		for i = 1, n do seq[i] = enc(m[i]) end
		parts[#parts + 1] = '"$seq":[' .. concat(seq, ",") .. "]"
	end
	return '{"mod":{' .. concat(parts, ",") .. '},"marks":{' .. concat(marks, ",") .. "}}"
end
local function encModList(l)
	local parts = { }
	for i = 1, #l do parts[i] = encMod(l[i]) end
	return "[" .. concat(parts, ",") .. "]"
end

local function isStore(s)
	return type(s) == "table" and (s._className == "ModDB" or s._className == "ModList")
end
-- The `data` table: every table as an object keyed by tostring(key) (sorted); a ModList as
-- {"$ModList#": n} (compact) or {"$ModList": [mods]} (full).
local function encData(t, full, depth)
	depth = depth or 0
	if depth > 6 then return '"<deep>"' end
	local keys = { }
	for k in pairs(t) do keys[#keys + 1] = k end
	sort(keys, function(a, b) return tostring(a) < tostring(b) end)
	local parts = { }
	for _, k in ipairs(keys) do
		local v = t[k]
		local s
		if isStore(v) then
			s = full and ('{"$ModList":' .. encModList(v) .. "}") or ('{"$ModList#":' .. #v .. "}")
		elseif type(v) == "table" then
			s = encData(v, full, depth + 1)
		else
			s = enc(v)
		end
		parts[#parts + 1] = jstr(type(k) == "number" and numKey(k) or tostring(k)) .. ":" .. s
	end
	return "{" .. concat(parts, ",") .. "}"
end

---------------------------------------------------------------------------
-- Nodes
---------------------------------------------------------------------------
local mainObj = main or __mainObject__.main
local tree = mainObj:LoadTree("3_26")
local ids = { }
for id in pairs(tree.nodes) do ids[#ids + 1] = id end
sort(ids)
local nodes = { }
for _, id in ipairs(ids) do nodes[#nodes + 1] = tree.nodes[id] end
local withStats = 0
local synth = { }
for _, node in ipairs(nodes) do
	if node.modList and #node.modList > 0 then
		withStats = withStats + 1
		if withStats % 7 == 0 then
			synth[#synth + 1] = setmetatable({ isTattoo = true }, { __index = node })
		end
		if withStats % 11 == 0 then
			synth[#synth + 1] = setmetatable({ conqueredBy = { id = 1, conqueror = { id = 1, type = "vaal" } } }, { __index = node })
		end
	end
end
for _, s in ipairs(synth) do nodes[#nodes + 1] = s end
-- Edge-case nodes the tree does not have: a weapon-flagged FLAG mod (arithmetic on a boolean in
-- the Melee -> Bow conversion), a numeric-string value, a nil source, two `Using*` conditions,
-- a `Condition:ConnectedTo` flag, a tattoo with attributes.
local cm = modLib.createMod
local function custom(id, ty, extra, mods)
	local n = { id = id, type = ty, modList = new("ModList"):ModList() }
	for k, v in pairs(extra) do n[k] = v end
	for _, m in ipairs(mods) do n.modList:AddMod(m) end
	nodes[#nodes + 1] = n
end
custom(900001, "Normal", { }, {
	cm("Str", "BASE", "12", "Tree:900001"),
	cm("Life", "INC", 5),
	cm("Damage", "INC", 10, "Tree:900001", 0, 0, { type = "Condition", var = "UsingSword" }, { type = "Condition", var = "UsingAxe" }),
	cm("Condition:ConnectedToSomething", "FLAG", true, "Tree:900001"),
	cm("Dex", "BASE", 30, "Tree:900001"),
	cm("Int", "BASE", 30, "Tree:900001"),
})
custom(900002, "Notable", { }, {
	cm("PhysicalDamage", "INC", 20, "Tree:900002", ModFlag.Axe),
	cm("Evasion", "INC", 15, "Tree:900002", 0, 0, { type = "Condition", var = "UsingClaw" }),
	cm("Str", "BASE", 45, "Tree:900002"),
	cm("ColdResist", "BASE", 12, "Tree:900002"),
	cm("ElementalResist", "BASE", 8, "Tree:900002"),
})
custom(900003, "Normal", { isTattoo = true }, {
	cm("Str", "BASE", 10, "Tree:900003"),
	cm("Life", "INC", 4, "Tree:900003"),
})
custom(900004, "Normal", { }, {
	cm("Speed", "FLAG", true, "Tree:900004", ModFlag.Axe),
})

local nodeJson = { }
for i, node in ipairs(nodes) do
	nodeJson[i] = fmt('{"id":%s,"type":%s,"tattoo":%s,"conq":%s,"mods":%s,"ks":%s}',
		jnum(node.id), jstr(tostring(node.type)), node.isTattoo and "true" or "false",
		node.conqueredBy and "true" or "false",
		node.modList and encModList(node.modList) or "null",
		node.keystoneMod and encMod(node.keystoneMod) or "null")
end

---------------------------------------------------------------------------
-- Funcs
---------------------------------------------------------------------------
local function runFunc(line)
	local ok, modList, extra = pcall(modLib.parseMod, line)
	if not ok then
		return fmt('{"line":%s,"parseError":%s}', jstr(line), jstr(tostring(modList)))
	end
	local m = modList and modList[1]
	local func = m and type(m.value) == "table" and m.value.func
	if type(func) ~= "function" then
		return fmt('{"line":%s,"noFunc":true}', jstr(line))
	end
	local calls, datas = { }, { }
	local data = { modSource = "Tree:61419" }
	local lastData = encData(data, false)
	for i, node in ipairs(nodes) do
		if node.modList then
			local out = new("ModList"):ModList()
			if node.type == "Keystone" then
				out:AddMod(node.keystoneMod)
			else
				out:AddList(node.modList)
			end
			local before = { }
			for j = 1, #out do before[j] = out[j] end
			local cok, err = pcall(func, node, out, data)
			local changed = not cok or #out ~= #before
			if not changed then
				for j = 1, #out do
					if out[j] ~= before[j] then changed = true break end
				end
			end
			if changed then
				local idx = { }
				for j, mm in ipairs(before) do idx[mm] = idx[mm] or j end
				local parts = { }
				for j = 1, #out do
					local mm = out[j]
					parts[j] = idx[mm] and ('{"$same":' .. idx[mm] .. "}") or encMod(mm)
				end
				calls[#calls + 1] = fmt('{"i":%d,"err":%s,"out":[%s]}', i,
					cok and "null" or jstr(tostring(err)), concat(parts, ","))
			end
			local d = encData(data, false)
			if d ~= lastData then
				datas[#datas + 1] = fmt('{"i":%d,"d":%s}', i, d)
				lastData = d
			end
		end
	end
	local out = new("ModList"):ModList()
	local cok, err = pcall(func, nil, out, data)
	local finish = fmt('{"err":%s,"out":%s,"data":%s}', cok and "null" or jstr(tostring(err)),
		encModList(out), encData(data, true))
	-- a second finish call with another source (getThreshold's shared `value.mod`, 6473-6474)
	data.modSource = "Tree:2"
	local out2 = new("ModList"):ModList()
	local cok2, err2 = pcall(func, nil, out2, data)
	local finish2 = fmt('{"err":%s,"out":%s,"data":%s}', cok2 and "null" or jstr(tostring(err2)),
		encModList(out2), encData(data, true))
	return fmt('{"line":%s,"name":%s,"type":%s,"calls":[%s],"datas":[%s],"finish":%s,"finish2":%s}',
		jstr(line), jstr(m.name), enc(m.value.type), concat(calls, ","), concat(datas, ","),
		finish, finish2)
end

local funcJson = { }
for i, line in ipairs(__mp7_lines) do
	funcJson[i] = runFunc(line)
end
return '{"nodes":[' .. concat(nodeJson, ",") .. '],"funcs":[' .. concat(funcJson, ",") .. "]}"
