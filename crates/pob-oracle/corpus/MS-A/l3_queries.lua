-- MS-A acceptance (crates/pob-oracle/tests/MS-A.rs): PoB's side of the ModStore query matrix.
--
-- Runs after a build has loaded and calculated (`build` = the BUILD mode). Collects the mod
-- stores of the main env (player/enemy/minion modDBs and their parents, every active skill's
-- skillModList, item mod lists, buff mod lists; plus synthetic MAX / MIN stores, FUP1),
-- serialises their mods (dump shape,
-- crates/pob-dump/FORMAT.md, so pob_gamedata::Mod deserialises them), runs the query matrix
-- (calc doc §9.4 L3) with pcall, and runs ScaleAddMod / ScaleAddList on real mods. Returns
-- one JSON string.
local env = build.calcsTab.mainEnv
local fmt, concat, sort = string.format, table.concat, table.sort
local floor, huge = math.floor, math.huge
local MAX_NAMES_PER_LIST = tonumber(__msa_max_names or 0) or 0

---------------------------------------------------------------------------
-- JSON (dump shape)
---------------------------------------------------------------------------
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

-- A generic Lua value. Class instances and functions become marker strings (both sides
-- decode the same JSON, so they still compare equal); cycles and depth are bounded.
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
	elseif tv == "function" then
		-- (`debug` is not available here; functions compare as one marker)
		return jstr("<function>")
	elseif tv == "table" then
		local mt = getmetatable(v)
		if mt and rawget(v, "Object") then
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

-- A mod: the standard keys, `$seq` tags, `$num` holey tags; any other key goes to `marks`
-- (ModDB writes `replaced` / `converted` into stored mods).
local stdKey = { name = true, type = true, value = true, flags = true, keywordFlags = true, source = true }
local function encMod(m)
	local n = 0
	while rawget(m, n + 1) ~= nil do n = n + 1 end
	local parts, marks = { }, { }
	local keys = { }
	for k in pairs(m) do keys[#keys + 1] = k end
	sort(keys, function(a, b) return tostring(a) < tostring(b) end)
	local nums = { }
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

---------------------------------------------------------------------------
-- Stores
---------------------------------------------------------------------------
local mods, modId = { }, { }
local function idOf(m)
	local id = modId[m]
	if not id then
		id = #mods + 1
		modId[m] = id
		mods[id] = encMod(m)
	end
	return id
end
local function isStore(s)
	return type(s) == "table" and (s._className == "ModDB" or s._className == "ModList")
end

local stores, storeId, storeObj = { }, { }, { }
local actors = { player = env.player, enemy = env.enemy, minion = env.minion }
local function addStore(s, label)
	if not isStore(s) then return nil end
	if storeId[s] then return storeId[s] end
	local id = #storeObj + 1
	storeId[s] = id
	storeObj[id] = s
	stores[id] = { label = label }
	if s.parent then addStore(s.parent, label .. ".parent") end
	return id
end
addStore(env.player.modDB, "player")
addStore(env.enemy.modDB, "enemy")
if env.minion then addStore(env.minion.modDB, "minion") end
local skillLists = { }
for i, skill in ipairs(env.player.activeSkillList) do
	local id = addStore(skill.skillModList, "skill" .. i)
	if id then skillLists[#skillLists + 1] = skill.skillModList end
	if skill.minion and skill.minion.modDB then addStore(skill.minion.modDB, "skill" .. i .. ".minion") end
	for j, buff in ipairs(skill.buffList or { }) do
		if isStore(buff.modList) then addStore(buff.modList, "skill" .. i .. ".buff" .. j) end
	end
end
if env.minion then
	for i, skill in ipairs(env.minion.activeSkillList or { }) do
		addStore(skill.skillModList, "minionSkill" .. i)
	end
end
for slot, item in pairs(env.player.itemList or { }) do
	if type(item) == "table" then
		addStore(item.modList, "item:" .. tostring(slot))
		if item.slotModList then
			addStore(item.slotModList[1], "item:" .. tostring(slot) .. ".slot1")
			addStore(item.slotModList[2], "item:" .. tostring(slot) .. ".slot2")
		end
	end
end

---------------------------------------------------------------------------
-- Synthetic MAX / MIN stores (FUP1, HYG2 exposed). The corpus has almost no MAX / MIN mods, so
-- Max / Min (ModStore.lua:251-271: Tabulate("MAX"/"MIN"), then EvalMod on each mod) would
-- only ever answer nil. These stores give them mods to find: untagged MAX / MIN mods of two
-- synthetic names and of Life (positive, negative, zero and fractional values; flags,
-- keywordFlags and sources that some cfgs match and others do not; a name whose MAX mods are
-- all negative, so Max stays nil through `val > (max or 0)`), held by a ModList child of each
-- actor's modDB (Tabulate walks the parent chain), a standalone ModDB and a ModDB child of it.
-- The query matrix below runs over them like over every other store.
---------------------------------------------------------------------------
local function synthMaxMin(s, tag)
	local function add(name, modType, value, source, flags, keywordFlags)
		s:AddMod(modLib.createMod(name, modType, value, source or ("Synth:" .. tag), flags or 0, keywordFlags or 0))
	end
	add("MsaMaxMinA", "MAX", 5)
	add("MsaMaxMinA", "MAX", 12.5)
	add("MsaMaxMinA", "MAX", -3)
	add("MsaMaxMinA", "MIN", 4)
	add("MsaMaxMinA", "MIN", -7.25)
	add("MsaMaxMinA", "MIN", 0)
	add("MsaMaxMinA", "MAX", 30, "Item:1:Synthetic")
	add("MsaMaxMinA", "MIN", -40, "Tree:1")
	add("MsaMaxMinA", "MAX", 50, nil, ModFlag.Attack)
	add("MsaMaxMinA", "MIN", -60, nil, ModFlag.Spell, KeywordFlag.Hit)
	add("MsaMaxMinA", "MAX", 70, nil, 0, KeywordFlag.Fire)
	add("MsaMaxMinA", "BASE", 1000)
	add("MsaMaxMinB", "MAX", -1)
	add("MsaMaxMinB", "MAX", -2)
	add("MsaMaxMinB", "MIN", 9)
	add("MsaMaxMinB", "MIN", 3, nil, ModFlag.Attack)
	add("Life", "MAX", 7)
	add("Life", "MIN", 2.5)
end
for _, key in ipairs({ "player", "enemy", "minion" }) do
	local a = actors[key]
	if a and isStore(a.modDB) then
		local l = new("ModList"):ModList(a.modDB)
		synthMaxMin(l, key)
		addStore(l, "synthMaxMin:" .. key)
	end
end
do
	local db = new("ModDB"):ModDB()
	synthMaxMin(db, "db")
	addStore(db, "synthMaxMin:db")
	local child = new("ModDB"):ModDB(db)
	child:AddMod(modLib.createMod("MsaMaxMinA", "MAX", 99, "Synth:child", 0, 0))
	child:AddMod(modLib.createMod("MsaMaxMinB", "MIN", -99, "Synth:child", 0, 0))
	addStore(child, "synthMaxMin:db.child")
end

-- Serialise every store.
-- string-keyed entries: numbers, booleans and strings as is, anything else as a (truthy) marker
local function tableOfScalars(t)
	local parts = { }
	local keys = { }
	for k, v in pairs(t or { }) do
		if type(k) == "string" then keys[#keys + 1] = k end
	end
	sort(keys)
	for _, k in ipairs(keys) do
		local v = t[k]
		local tv = type(v)
		parts[#parts + 1] = jstr(k) .. ":" .. ((tv == "number" or tv == "boolean" or tv == "string") and enc(v) or jstr("<" .. tv .. ">"))
	end
	return "{" .. concat(parts, ",") .. "}"
end
local function actorKey(s)
	for k, a in pairs(actors) do
		if rawget(s, "actor") == a then return k end
	end
	return "none"
end
local storeJson = { }
for id, s in ipairs(storeObj) do
	local parts = { '"label":' .. jstr(stores[id].label), '"kind":' .. jstr(s._className),
		'"parent":' .. (s.parent and tostring(storeId[s.parent]) or "null"),
		'"actor":' .. jstr(actorKey(s)),
		'"conditions":' .. tableOfScalars(s.conditions), '"multipliers":' .. tableOfScalars(s.multipliers) }
	if s._className == "ModDB" then
		local names = { }
		for name in pairs(s.mods) do names[#names + 1] = name end
		sort(names)
		local buckets = { }
		for _, name in ipairs(names) do
			local ids = { }
			for i, m in ipairs(s.mods[name]) do ids[i] = idOf(m) end
			buckets[#buckets + 1] = "[" .. jstr(name) .. ",[" .. concat(ids, ",") .. "]]"
		end
		parts[#parts + 1] = '"buckets":[' .. concat(buckets, ",") .. "]"
	else
		local ids = { }
		for i, m in ipairs(s) do ids[i] = idOf(m) end
		parts[#parts + 1] = '"list":[' .. concat(ids, ",") .. "]"
	end
	storeJson[id] = "{" .. concat(parts, ",") .. "}"
end

---------------------------------------------------------------------------
-- Cfgs
---------------------------------------------------------------------------
local cfgs = { { name = "nil", cfg = nil } }
local main = env.player.mainSkill
if main then
	for _, k in ipairs({ "skillCfg", "weapon1Cfg", "weapon2Cfg", "dotCfg", "bleedCfg", "poisonCfg", "igniteCfg" }) do
		if type(main[k]) == "table" then cfgs[#cfgs + 1] = { name = k, cfg = main[k] } end
	end
end
cfgs[#cfgs + 1] = { name = "srcItem", cfg = { source = "Item" } }
cfgs[#cfgs + 1] = { name = "srcTree", cfg = { source = "Tree", flags = ModFlag.Attack } }
cfgs[#cfgs + 1] = { name = "hitAttack", cfg = { flags = bit.bor(ModFlag.Attack, ModFlag.Hit, ModFlag.Weapon), keywordFlags = KeywordFlag.Hit } }
cfgs[#cfgs + 1] = { name = "spellAll", cfg = { flags = bit.bor(ModFlag.Spell, ModFlag.Hit, ModFlag.Area), keywordFlags = bit.bor(KeywordFlag.Spell, KeywordFlag.Hit, KeywordFlag.Fire, KeywordFlag.Cold, KeywordFlag.Lightning) } }
local cfgJson = { }
for i, c in ipairs(cfgs) do
	local cfg = c.cfg
	if cfg == nil then
		cfgJson[i] = '{"name":"nil","nil":true}'
	else
		cfgJson[i] = "{" .. concat({
			'"name":' .. jstr(c.name),
			'"flags":' .. enc(cfg.flags),
			'"keywordFlags":' .. enc(cfg.keywordFlags),
			'"source":' .. enc(cfg.source),
			'"skillName":' .. enc(cfg.skillName),
			'"summonSkillName":' .. enc(cfg.summonSkillName),
			'"skillStats":' .. (type(cfg.skillStats) == "table" and tableOfScalars(cfg.skillStats) or "null"),
		}, ",") .. "}"
	end
end

---------------------------------------------------------------------------
-- Queries
---------------------------------------------------------------------------
local results = { }
local function rec(storeIdx, q, cfgIdx, names, res)
	results[#results + 1] = "[" .. storeIdx .. "," .. jstr(q) .. "," .. cfgIdx .. ",[" .. names .. "]," .. res .. "]"
end
local function encRes(ok, v, kind)
	if not ok then return '{"err":' .. jstr(tostring(v)) .. "}" end
	if kind == "tab" then
		local rows = { }
		for i, row in ipairs(v) do rows[i] = "[" .. idOf(row.mod) .. "," .. enc(row.value) .. "]" end
		return "[" .. concat(rows, ",") .. "]"
	elseif kind == "list" then
		local items = { }
		for i, x in ipairs(v) do items[i] = enc(x) end
		return "[" .. concat(items, ",") .. "]"
	elseif kind == "flag" then
		return v and "true" or "false"
	end
	return enc(v)
end

-- pcall + encode (pcall's results must be the last arguments of a call to survive)
local function pq(kind, f, ...)
	local ok, v = pcall(f, ...)
	return encRes(ok, v, kind)
end

local function chainNames(s, own)
	local set, list = { }, { }
	local function add(name)
		if not set[name] then set[name] = true; list[#list + 1] = name end
	end
	local cur, depth = s, 0
	while cur do
		if cur._className == "ModDB" then
			for name in pairs(cur.mods) do add(name) end
		else
			for _, m in ipairs(cur) do add(m.name) end
		end
		if own then break end
		cur = cur.parent
		depth = depth + 1
	end
	sort(list)
	return list
end

local multi = { }
for id, s in ipairs(storeObj) do
	-- ModLists with a parent: the list's own names (the query still walks the parent chain).
	local names = chainNames(s, s._className == "ModList" and s.parent ~= nil)
	if MAX_NAMES_PER_LIST > 0 and #names > MAX_NAMES_PER_LIST then
		local cut = { }
		for i = 1, MAX_NAMES_PER_LIST do cut[i] = names[i] end
		names = cut
	end
	-- every cfg on the ModDBs; a subset on the (many) ModLists
	local listCfg = { ["nil"] = true, skillCfg = true, srcItem = true, hitAttack = true }
	for ci, c in ipairs(cfgs) do
		if s._className == "ModDB" or listCfg[c.name] then
		local cfg = c.cfg
		local function run(nameList)
			local nj = { }
			for i, n in ipairs(nameList) do nj[i] = jstr(n) end
			local nstr = concat(nj, ",")
			local u = table.unpack or unpack
			rec(id, "SumBASE", ci, nstr, pq(nil, s.Sum, s, "BASE", cfg, u(nameList)))
			rec(id, "SumINC", ci, nstr, pq(nil, s.Sum, s, "INC", cfg, u(nameList)))
			rec(id, "More", ci, nstr, pq(nil, s.More, s, cfg, u(nameList)))
			rec(id, "Flag", ci, nstr, pq("flag", s.Flag, s, cfg, u(nameList)))
			rec(id, "Override", ci, nstr, pq(nil, s.Override, s, cfg, u(nameList)))
			rec(id, "List", ci, nstr, pq("list", s.List, s, cfg, u(nameList)))
			rec(id, "Tabulate", ci, nstr, pq("tab", s.Tabulate, s, nil, cfg, u(nameList)))
			rec(id, "HasModBASE", ci, nstr, pq("flag", s.HasMod, s, "BASE", cfg, u(nameList)))
			rec(id, "Max", ci, nstr, pq(nil, s.Max, s, cfg, u(nameList)))
			rec(id, "Min", ci, nstr, pq(nil, s.Min, s, cfg, u(nameList)))
		end
		for _, name in ipairs(names) do run({ name }) end
		-- multi-name queries: consecutive pairs and one long list
		for i = 1, #names - 1, 7 do run({ names[i], names[i + 1] }) end
		if #names > 2 then
			local long = { }
			for i = 1, #names, math.max(1, floor(#names / 12)) do long[#long + 1] = names[i] end
			run(long)
		end
		-- conditions / multipliers
		local condVars, multVars = { }, { }
		local cur = s
		while cur do
			for k in pairs(cur.conditions or { }) do if type(k) == "string" then condVars[k] = true end end
			for k in pairs(cur.multipliers or { }) do if type(k) == "string" then multVars[k] = true end end
			cur = cur.parent
		end
		for _, name in ipairs(names) do
			local v = name:match("^Condition:(.*)$")
			if v then condVars[v] = true end
			v = name:match("^Multiplier:(.*)$")
			if v then multVars[v] = true end
		end
		for _, set in ipairs({ condVars, multVars }) do
			local vars = { }
			for k in pairs(set) do vars[#vars + 1] = k end
			sort(vars)
			for _, var in ipairs(vars) do
				if set == condVars then
					rec(id, "GetCondition", ci, jstr(var), pq("flag", s.GetCondition, s, var, cfg, false))
					rec(id, "GetConditionNoMod", ci, jstr(var), pq("flag", s.GetCondition, s, var, cfg, true))
				else
					rec(id, "GetMultiplier", ci, jstr(var), pq(nil, s.GetMultiplier, s, var, cfg, false))
					rec(id, "GetMultiplierNoMod", ci, jstr(var), pq(nil, s.GetMultiplier, s, var, cfg, true))
				end
			end
		end
		end
	end
end

-- GetStat on the actor stores.
local actorJson = { }
for key, a in pairs(actors) do
	if a and a.modDB then
		local id = storeId[a.modDB]
		local skills = { }
		for i, skill in ipairs(a.activeSkillList or { }) do
			local buffNames = { }
			for j, buff in ipairs(skill.buffList or { }) do buffNames[j] = enc(buff.name) end
			skills[i] = "{" .. concat({
				'"hasReservation":' .. ((skill.skillTypes and skill.skillTypes[SkillType.HasReservation]) and "true" or "false"),
				'"disable":' .. ((skill.skillFlags and skill.skillFlags.disable) and "true" or "false"),
				'"buffNames":[' .. concat(buffNames, ",") .. "]",
				'"ManaReservedBase":' .. enc(skill.skillData and skill.skillData.ManaReservedBase),
				'"LifeReservedBase":' .. enc(skill.skillData and skill.skillData.LifeReservedBase),
			}, ",") .. "}"
		end
		actorJson[#actorJson + 1] = jstr(key) .. ':{"store":' .. tostring(id) .. ',"output":' .. tableOfScalars(a.output)
			.. ',"ManaEfficiency":' .. enc(a.ManaEfficiency) .. ',"hasSkillList":' .. tostring(a.activeSkillList ~= nil) .. ',"skills":[' .. concat(skills, ",") .. "]}"
		local stats = { "ManaReservedPercent", "LifeReservedPercent", "ManaUnreserved" }
		for k, v in pairs(a.output or { }) do
			if type(k) == "string" and type(v) == "number" then stats[#stats + 1] = k end
		end
		sort(stats)
		for ci, c in ipairs(cfgs) do
			for _, stat in ipairs(stats) do
				rec(id, "GetStat", ci, jstr(stat), pq(nil, a.modDB.GetStat, a.modDB, stat, c.cfg))
			end
		end
	end
end

---------------------------------------------------------------------------
-- ScaleAddMod / ScaleAddList (aura scaling) on real mods
---------------------------------------------------------------------------
local function dumpStore(s)
	if s._className == "ModDB" then
		local names = { }
		for name in pairs(s.mods) do names[#names + 1] = name end
		sort(names)
		local buckets = { }
		for _, name in ipairs(names) do
			local ms = { }
			for i, m in ipairs(s.mods[name]) do ms[i] = encMod(m) end
			buckets[#buckets + 1] = "[" .. jstr(name) .. ",[" .. concat(ms, ",") .. "]]"
		end
		return '{"buckets":[' .. concat(buckets, ",") .. "]}"
	end
	local ms = { }
	for i, m in ipairs(s) do ms[i] = encMod(m) end
	return '{"list":[' .. concat(ms, ",") .. "]}"
end
local scales = { 0.5, 1.37, 2.25, 0.123, -0.4, 1 }
local scaleRes = { }
local scaleSrc = { }
-- every mod in the player modDB and in every buff list
for name, list in pairs(env.player.modDB.mods) do
	for _, m in ipairs(list) do scaleSrc[#scaleSrc + 1] = m end
end
for _, skill in ipairs(env.player.activeSkillList) do
	for _, buff in ipairs(skill.buffList or { }) do
		if isStore(buff.modList) then
			for _, m in ipairs(buff.modList) do scaleSrc[#scaleSrc + 1] = m end
		end
	end
end
sort(scaleSrc, function(a, b) return idOf(a) < idOf(b) end)
for _, m in ipairs(scaleSrc) do
	for si, scale in ipairs(scales) do
		local ml = new("ModList"):ModList()
		local ok, e = pcall(ml.ScaleAddMod, ml, m, scale)
		local out = ok and (ml[1] and encMod(ml[1]) or "null") or ('{"err":' .. jstr(tostring(e)) .. "}")
		scaleRes[#scaleRes + 1] = "[" .. idOf(m) .. "," .. jnum(scale) .. "," .. out .. "]"
	end
end
-- ScaleAddList with replace = true: into a ModDB, a ModList, and a ModDB child of a ModDB parent
local listRes = { }
local srcLists = { }
for _, skill in ipairs(env.player.activeSkillList) do
	for _, buff in ipairs(skill.buffList or { }) do
		if isStore(buff.modList) and buff.modList._className == "ModList" and #buff.modList > 0 then
			srcLists[#srcLists + 1] = buff.modList
		end
	end
	if #skill.skillModList > 0 then srcLists[#srcLists + 1] = skill.skillModList end
end
for li, list in ipairs(srcLists) do
	local ids = { }
	local plain = { }
	for i, m in ipairs(list) do ids[i] = idOf(m); plain[i] = m end
	for _, scale in ipairs({ 1.5, 0.5, 1 }) do
		local db = new("ModDB"):ModDB()
		db:AddList(plain)
		local ok1, e1 = pcall(db.ScaleAddList, db, plain, scale, true)
		local ok1b, e1b = pcall(db.ScaleAddList, db, plain, scale, true)
		local ml = new("ModList"):ModList()
		ml:AddList(plain)
		local ok2, e2 = pcall(ml.ScaleAddList, ml, plain, scale, true)
		local p = new("ModDB"):ModDB()
		p:AddList(plain)
		local c = new("ModDB"):ModDB(p)
		local ok3, e3 = pcall(c.ScaleAddList, c, plain, scale, true)
		local cl = new("ModList"):ModList(p)
		local ok4, e4 = pcall(cl.ScaleAddList, cl, plain, scale, false)
		listRes[#listRes + 1] = "{" .. concat({
			'"ids":[' .. concat(ids, ",") .. "]",
			'"scale":' .. jnum(scale),
			'"db":' .. (ok1 and ok1b and dumpStore(db) or jstr("err " .. tostring(e1) .. tostring(e1b))),
			'"ml":' .. (ok2 and dumpStore(ml) or jstr("err " .. tostring(e2))),
			'"parent":' .. dumpStore(p),
			'"child":' .. (ok3 and dumpStore(c) or jstr("err " .. tostring(e3))),
			'"childList":' .. (ok4 and dumpStore(cl) or jstr("err " .. tostring(e4))),
		}, ",") .. "}"
	end
end

---------------------------------------------------------------------------
-- createMod paths: NewMod / ReplaceMod / ConvertMod / MergeNewMod (need modLib.createMod)
---------------------------------------------------------------------------
local picked = { }
for _, m in ipairs(scaleSrc) do
	if m[1] == nil and type(m.value) == "number" and type(m.source) == "string" and type(m.flags) == "number" then
		picked[#picked + 1] = m
		if #picked >= 40 then break end
	end
end
local cmRes = { }
for k, m in ipairs(picked) do
	local p = new("ModDB"):ModDB()
	p:AddList(picked)
	local c = new("ModDB"):ModDB(p)
	local l = new("ModList"):ModList(p)
	local ok, e = pcall(function()
		c:ReplaceMod(m.name, m.type, m.value + 1, m.source, m.flags, m.keywordFlags)
		c:ReplaceMod(m.name, m.type, m.value + 2, m.source, m.flags, m.keywordFlags)
		c:ConvertMod(m.name, m.name .. "Conv", m.type, m.value * 2, m.source, m.flags, m.keywordFlags)
		l:NewMod(m.name, m.type, 5, m.source, m.flags, m.keywordFlags)
		l:ConvertMod(m.name, m.name .. "Conv2", m.type, 3, m.source, m.flags, m.keywordFlags)
		l:NewMod(m.name, m.type, 5, m.source, m.flags, m.keywordFlags)
		l:MergeNewMod(m.name, m.type, 7, m.source, m.flags, m.keywordFlags)
		l:MergeNewMod(m.name .. "X", "BASE", 1, m.source)
		l:ReplaceMod(m.name, m.type, 9, m.source, m.flags, m.keywordFlags)
	end)
	cmRes[#cmRes + 1] = "{" .. concat({
		'"pick":' .. k,
		'"err":' .. (ok and "null" or jstr(tostring(e))),
		'"parent":' .. dumpStore(p),
		'"child":' .. dumpStore(c),
		'"list":' .. dumpStore(l),
	}, ",") .. "}"
end
local pickedIds = { }
for i, m in ipairs(picked) do pickedIds[i] = idOf(m) end

return "{" .. concat({
	'"picked":[' .. concat(pickedIds, ",") .. "]",
	'"createMod":[' .. concat(cmRes, ",") .. "]",
	'"mods":[' .. concat(mods, ",") .. "]",
	'"stores":[' .. concat(storeJson, ",") .. "]",
	'"cfgs":[' .. concat(cfgJson, ",") .. "]",
	'"actors":{' .. concat(actorJson, ",") .. "}",
	'"results":[' .. concat(results, ",") .. "]",
	'"scale":[' .. concat(scaleRes, ",") .. "]",
	'"scaleList":[' .. concat(listRes, ",") .. "]",
}, ",") .. "}"
