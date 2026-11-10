-- MS-B acceptance (crates/pob-oracle/tests/MS-B.rs): PoB's side of the full L3 query matrix
-- (calc doc §9.4 L3, §11.1 row MS-B), i.e. including every tag path of ModStore:EvalMod.
--
-- Runs after a build has loaded and calculated (`build` = the BUILD mode). On top of what the
-- MS-A script collects (the main env's mod stores and their mods in the dump shape), it records
-- everything EvalMod reads besides the store itself, so the Rust side can rebuild it:
--   * every actor reachable from the env and from the stores (`store.actor`), with its
--     player/enemy/parent/minion/partyMembers links, modDB, weaponData1/2 scalars (countsAsAll1H,
--     Added<Cond>), itemList (slot -> item), minionData (its data.minions key), output;
--   * every item of those itemLists (name, type, rarity, corrupted, shaper, elder, and the
--     enabled lines of its explicit/enchant/scourge/implicit/crucible mod line lists);
--   * the cfgs with every key EvalMod reads (skillCond read through its __index chain for every
--     Condition var of the dumped mods, skillTypes, skillGrantedEffect / skillGem ids, skillPart,
--     skillDist, skillStats, slotName, socket fields, item, baseFlags, actor).
-- Then it runs the query matrix with pcall (calc doc §9.4 L3: every mod name × cfg × query).
-- Returns one JSON string.
local env = build.calcsTab.mainEnv
local fmt, concat, sort = string.format, table.concat, table.sort
local floor, huge = math.floor, math.huge

---------------------------------------------------------------------------
-- JSON (dump shape; same encoder as corpus/MS-A/l3_queries.lua)
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

-- string-keyed scalars as is, anything else as a (truthy) marker
local function tableOfScalars(t)
	local parts, keys = { }, { }
	for k in pairs(t or { }) do
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

---------------------------------------------------------------------------
-- Registries: mods, stores, actors, items
---------------------------------------------------------------------------
local mods, modId, modObj = { }, { }, { }
local function idOf(m)
	local id = modId[m]
	if not id then
		id = #mods + 1
		modId[m] = id
		modObj[id] = m
		mods[id] = encMod(m)
	end
	return id
end
local function isStore(s)
	return type(s) == "table" and (s._className == "ModDB" or s._className == "ModList")
end

local stores, storeId, storeObj = { }, { }, { }
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

local itemId, itemObj = { }, { }
local function addItem(it)
	if type(it) ~= "table" then return nil end
	if itemId[it] then return itemId[it] end
	local id = #itemObj + 1
	itemId[it] = id
	itemObj[id] = it
	return id
end

-- An actor is any non-empty table used as `store.actor` or linked from one. `{ }` (a store made
-- without a parent) has no fields at all: EvalMod reads nil from it, the Rust side models it as
-- "no actor".
local actorId, actorObj = { }, { }
local actorLinks = { "player", "enemy", "parent", "minion", "partyMembers" }
local function addActor(a, label)
	if type(a) ~= "table" or next(a) == nil then return nil end
	if actorId[a] then return actorId[a] end
	local id = #actorObj + 1
	actorId[a] = id
	actorObj[id] = a
	if isStore(rawget(a, "modDB")) then addStore(a.modDB, (label or ("actor" .. id)) .. ".modDB") end
	for _, k in ipairs(actorLinks) do addActor(rawget(a, k), "actor" .. id .. "." .. k) end
	return id
end

addStore(env.player.modDB, "player")
addStore(env.enemy.modDB, "enemy")
if env.minion then addStore(env.minion.modDB, "minion") end
addActor(env.player, "player")
addActor(env.enemy, "enemy")
if env.minion then addActor(env.minion, "minion") end
local skillOfStore = { }
local function addSkills(list, prefix)
	for i, skill in ipairs(list or { }) do
		local id = addStore(skill.skillModList, prefix .. i)
		if id then skillOfStore[id] = skill end
		if skill.minion then
			addActor(skill.minion, prefix .. i .. ".minion")
			if skill.minion.modDB then addStore(skill.minion.modDB, prefix .. i .. ".minion") end
		end
		for j, buff in ipairs(skill.buffList or { }) do
			if isStore(buff.modList) then addStore(buff.modList, prefix .. i .. ".buff" .. j) end
		end
	end
end
addSkills(env.player.activeSkillList, "skill")
if env.minion then addSkills(env.minion.activeSkillList, "minionSkill") end
for slot, item in pairs(env.player.itemList or { }) do
	if type(item) == "table" then
		addStore(item.modList, "item:" .. tostring(slot))
		if item.slotModList then
			addStore(item.slotModList[1], "item:" .. tostring(slot) .. ".slot1")
			addStore(item.slotModList[2], "item:" .. tostring(slot) .. ".slot2")
		end
	end
end
-- fixpoint: the actor of every store (which may add stores, whose actors are added in turn)
do
	local i = 1
	while i <= #storeObj do
		addActor(rawget(storeObj[i], "actor"))
		i = i + 1
	end
end

-- reverse data maps (identity)
local minionKey, gemKey = { }, { }
for k, v in pairs(data.minions or { }) do minionKey[v] = k end
for k, v in pairs(data.gems or { }) do gemKey[v] = k end

---------------------------------------------------------------------------
-- Serialise stores
---------------------------------------------------------------------------
local storeJson = { }
for id, s in ipairs(storeObj) do
	local a = rawget(s, "actor")
	local parts = { '"label":' .. jstr(stores[id].label), '"kind":' .. jstr(s._className),
		'"parent":' .. (s.parent and tostring(storeId[s.parent]) or "null"),
		'"actor":' .. (actorId[a] and tostring(actorId[a]) or "null"),
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
-- Serialise actors and items
---------------------------------------------------------------------------
local actorJson = { }
for id, a in ipairs(actorObj) do
	local parts = { '"id":' .. id, '"modDB":' .. (storeId[rawget(a, "modDB")] and tostring(storeId[a.modDB]) or "null") }
	for _, k in ipairs(actorLinks) do
		local v = rawget(a, k)
		parts[#parts + 1] = jstr(k) .. ":" .. (actorId[v] and tostring(actorId[v]) or "null")
	end
	for _, k in ipairs({ "weaponData1", "weaponData2" }) do
		local w = rawget(a, k)
		parts[#parts + 1] = jstr(k) .. ":" .. (type(w) == "table" and tableOfScalars(w) or "null")
	end
	local il = rawget(a, "itemList")
	if type(il) == "table" then
		local slots, sp = { }, { }
		for slot, item in pairs(il) do
			if type(slot) == "string" and type(item) == "table" then slots[#slots + 1] = slot end
		end
		sort(slots)
		for _, slot in ipairs(slots) do sp[#sp + 1] = jstr(slot) .. ":" .. addItem(il[slot]) end
		parts[#parts + 1] = '"itemList":{' .. concat(sp, ",") .. "}"
	else
		parts[#parts + 1] = '"itemList":null'
	end
	local md = rawget(a, "minionData")
	parts[#parts + 1] = '"minionData":' .. (md and enc(minionKey[md] or "<unknown>") or "null")
	parts[#parts + 1] = '"minionHasTags":' .. tostring(md ~= nil and md.monsterTags ~= nil)
	parts[#parts + 1] = '"output":' .. tableOfScalars(rawget(a, "output"))
	parts[#parts + 1] = '"hasOutput":' .. tostring(type(rawget(a, "output")) == "table")
	parts[#parts + 1] = '"ManaEfficiency":' .. enc(rawget(a, "ManaEfficiency"))
	parts[#parts + 1] = '"hasSkillList":' .. tostring(rawget(a, "activeSkillList") ~= nil)
	local skills = { }
	for i, skill in ipairs(rawget(a, "activeSkillList") or { }) do
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
	parts[#parts + 1] = '"skills":[' .. concat(skills, ",") .. "]"
	actorJson[id] = "{" .. concat(parts, ",") .. "}"
end

---------------------------------------------------------------------------
-- Cfgs
---------------------------------------------------------------------------
-- Condition vars of every dumped mod (EvalMod reads `cfg.skillCond[var]` only for these).
local condVars, condVarList = { }, { }
local function addCondVar(v)
	if type(v) == "string" and not condVars[v] then condVars[v] = true; condVarList[#condVarList + 1] = v end
end
for _, m in ipairs(modObj) do
	for _, tag in ipairs(m) do
		if type(tag) == "table" and tag.type == "Condition" then
			addCondVar(tag.var)
			for _, v in pairs(tag.varList or { }) do addCondVar(v) end
		end
	end
end

-- The distinct tags of the synthetic probes (see "Synthetic tag probes" below); collected here
-- so that their Condition vars are part of the skillCond dump.
local tags = { }
if __msb_synthetic then
	local seen = { }
	local function scan(m, depth)
		if type(m) ~= "table" or depth > 3 then return end
		for _, tag in ipairs(m) do
			if type(tag) == "table" and type(tag.type) == "string" then
				local k = enc(tag)
				if not seen[k] then seen[k] = true; tags[#tags + 1] = { k, tag } end
				if tag.type == "Condition" then
					addCondVar(tag.var)
					for _, v in pairs(tag.varList or { }) do addCondVar(v) end
				end
			end
		end
		if type(m.value) == "table" and type(m.value.mod) == "table" then scan(m.value.mod, depth + 1) end
	end
	for _, e in pairs(modLib.parseModCache) do
		if type(e) == "table" and type(e[1]) == "table" then
			for _, m in ipairs(e[1]) do scan(m, 0) end
		end
	end
	for _, sk in pairs(data.skills) do
		for _, map in pairs(sk.statMap or { }) do
			if type(map) == "table" then
				for _, mg in ipairs(map) do
					if type(mg) == "table" and mg.name then scan(mg, 0)
					elseif type(mg) == "table" then for _, m in ipairs(mg) do scan(m, 0) end end
				end
			end
		end
		for _, m in ipairs(sk.baseMods or { }) do scan(m, 0) end
	end
	sort(tags, function(a, b) return a[1] < b[1] end)
end

local cfgs = { { name = "nil", cfg = nil } }
local main = env.player.mainSkill
if main then
	for _, k in ipairs({ "skillCfg", "weapon1Cfg", "weapon2Cfg", "dotCfg", "bleedCfg", "poisonCfg", "igniteCfg" }) do
		if type(main[k]) == "table" then cfgs[#cfgs + 1] = { name = k, cfg = main[k] } end
	end
	-- skillDist variants of the main skillCfg (DistanceRamp / MeleeProximity branches)
	if type(main.skillCfg) == "table" then
		for _, d in ipairs({ 5, 15.5, 22, 45 }) do
			local c = copyTable(main.skillCfg, true)
			c.skillDist = d
			cfgs[#cfgs + 1] = { name = "skillCfgDist" .. d, cfg = c }
		end
	end
end
cfgs[#cfgs + 1] = { name = "srcItem", cfg = { source = "Item" } }
cfgs[#cfgs + 1] = { name = "srcTree", cfg = { source = "Tree", flags = ModFlag.Attack } }
cfgs[#cfgs + 1] = { name = "hitAttack", cfg = { flags = bit.bor(ModFlag.Attack, ModFlag.Hit, ModFlag.Weapon), keywordFlags = KeywordFlag.Hit } }
cfgs[#cfgs + 1] = { name = "spellAll", cfg = { flags = bit.bor(ModFlag.Spell, ModFlag.Hit, ModFlag.Area), keywordFlags = bit.bor(KeywordFlag.Spell, KeywordFlag.Hit, KeywordFlag.Fire, KeywordFlag.Cold, KeywordFlag.Lightning) } }
local nGlobalCfgs = #cfgs
-- every skill's own cfgs, used on that skill's skillModList only
local ownCfgs = { }
for sid, skill in pairs(skillOfStore) do
	ownCfgs[sid] = { }
	for _, k in ipairs({ "skillCfg", "weapon1Cfg", "weapon2Cfg" }) do
		if type(skill[k]) == "table" and not (skill == main) then
			cfgs[#cfgs + 1] = { name = stores[sid].label .. "." .. k, cfg = skill[k] }
			ownCfgs[sid][#ownCfgs[sid] + 1] = #cfgs
		end
	end
end

local function keysOf(t, wantNumber)
	local ks = { }
	for k, v in pairs(t or { }) do
		if v and ((wantNumber and type(k) == "number") or (not wantNumber and type(k) == "string")) then ks[#ks + 1] = k end
	end
	sort(ks)
	return ks
end
local cfgJson = { }
for i, c in ipairs(cfgs) do
	local cfg = c.cfg
	if cfg == nil then
		cfgJson[i] = '{"name":"nil","nil":true}'
	else
		local sc = "null"
		if type(cfg.skillCond) == "table" then
			local p = { }
			local seen = { }
			local function put(k)
				if seen[k] then return end
				seen[k] = true
				local v = cfg.skillCond[k]
				if v ~= nil then
					local tv = type(v)
					p[#p + 1] = jstr(k) .. ":" .. ((tv == "number" or tv == "boolean" or tv == "string") and enc(v) or jstr("<" .. tv .. ">"))
				end
			end
			for _, k in ipairs(keysOf(cfg.skillCond, false)) do put(k) end
			for _, k in ipairs(condVarList) do put(k) end
			sc = "{" .. concat(p, ",") .. "}"
		end
		local st = "null"
		if type(cfg.skillTypes) == "table" then
			st = "[" .. concat(keysOf(cfg.skillTypes, true), ",") .. "]"
		end
		local bf = "null"
		if type(cfg.baseFlags) == "table" then
			local ks = { }
			for _, k in ipairs(keysOf(cfg.baseFlags, false)) do ks[#ks + 1] = jstr(k) end
			bf = "[" .. concat(ks, ",") .. "]"
		end
		local ge = cfg.skillGrantedEffect
		local gem = cfg.skillGem
		cfgJson[i] = "{" .. concat({
			'"name":' .. jstr(c.name),
			'"flags":' .. enc(cfg.flags),
			'"keywordFlags":' .. enc(cfg.keywordFlags),
			'"source":' .. enc(cfg.source),
			'"actor":' .. enc(type(cfg.actor) == "string" and cfg.actor or nil),
			'"skillName":' .. enc(cfg.skillName),
			'"summonSkillName":' .. enc(cfg.summonSkillName),
			'"skillStats":' .. (type(cfg.skillStats) == "table" and tableOfScalars(cfg.skillStats) or "null"),
			'"skillCond":' .. sc,
			'"skillTypes":' .. st,
			'"skillGrantedEffect":' .. (type(ge) == "table" and enc(ge.id) or "null"),
			'"skillGrantedEffectIsData":' .. tostring(type(ge) == "table" and data.skills[ge.id] == ge),
			'"skillGem":' .. (type(gem) == "table" and enc(gemKey[gem] or "<unknown>") or "null"),
			'"skillPart":' .. enc(cfg.skillPart),
			'"skillDist":' .. enc(cfg.skillDist),
			'"slotName":' .. enc(cfg.slotName),
			'"socketColor":' .. enc(cfg.socketColor),
			'"socketNum":' .. enc(cfg.socketNum),
			'"strengthGems":' .. enc(cfg.strengthGems),
			'"dexterityGems":' .. enc(cfg.dexterityGems),
			'"intelligenceGems":' .. enc(cfg.intelligenceGems),
			'"item":' .. (type(cfg.item) == "table" and tostring(addItem(cfg.item)) or "null"),
			'"baseFlags":' .. bf,
		}, ",") .. "}"
	end
end

-- The mod lines Item:FindModifierSubstring (Item.lua:289-333) reads, already filtered by its
-- L308 `not v.disabled and self:CheckModLineVariant(v)` (variant selection is IT-B's, not
-- EvalMod's): the line text per list, so the Rust item answers ItemCondition `searchCond` tags.
local modLineLists = { "explicitModLines", "enchantModLines", "scourgeModLines", "implicitModLines", "crucibleModLines" }
local function encModLines(it, key)
	local lines = { }
	local list = rawget(it, key)
	if type(list) == "table" then
		for _, v in ipairs(list) do
			if type(v) == "table" and not v.disabled and (not it.CheckModLineVariant or it:CheckModLineVariant(v)) then
				lines[#lines + 1] = enc(v.line)
			end
		end
	end
	return '"' .. key .. '":[' .. concat(lines, ",") .. "]"
end
local itemJson = { }
for id, it in ipairs(itemObj) do
	local fields = {
		'"name":' .. enc(it.name),
		'"type":' .. enc(it.type),
		'"rarity":' .. enc(it.rarity),
		'"corrupted":' .. enc(it.corrupted),
		'"shaper":' .. enc(it.shaper),
		'"elder":' .. enc(it.elder),
	}
	for _, key in ipairs(modLineLists) do fields[#fields + 1] = encModLines(it, key) end
	itemJson[id] = "{" .. concat(fields, ",") .. "}"
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
local function pq(kind, f, ...)
	local ok, v = pcall(f, ...)
	return encRes(ok, v, kind)
end

local function chainNames(s, own)
	local set, list = { }, { }
	local function add(name)
		if not set[name] then set[name] = true; list[#list + 1] = name end
	end
	local cur = s
	while cur do
		if cur._className == "ModDB" then
			for name in pairs(cur.mods) do add(name) end
		else
			for _, m in ipairs(cur) do add(m.name) end
		end
		if own then break end
		cur = cur.parent
	end
	sort(list)
	return list
end

local u = table.unpack or unpack
-- `__msb_no_queries = true` (benches/MS-B.rs): dump the state only
local nStores = __msb_no_queries and 0 or #storeObj
for id = 1, nStores do
	local s = storeObj[id]
	local names = chainNames(s, s._className == "ModList" and s.parent ~= nil)
	local listCfg = { ["nil"] = true, skillCfg = true, srcItem = true, hitAttack = true }
	local cfgIdx = { }
	for ci = 1, nGlobalCfgs do
		if s._className == "ModDB" or listCfg[cfgs[ci].name] then cfgIdx[#cfgIdx + 1] = ci end
	end
	for _, ci in ipairs(ownCfgs[id] or { }) do cfgIdx[#cfgIdx + 1] = ci end
	for _, ci in ipairs(cfgIdx) do
		local cfg = cfgs[ci].cfg
		local function run(nameList)
			local nj = { }
			for i, n in ipairs(nameList) do nj[i] = jstr(n) end
			local nstr = concat(nj, ",")
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
		for i = 1, #names - 1, 7 do run({ names[i], names[i + 1] }) end
		if #names > 2 then
			local long = { }
			for i = 1, #names, math.max(1, floor(#names / 12)) do long[#long + 1] = names[i] end
			run(long)
		end
		local cv, mv = { }, { }
		local cur = s
		while cur do
			for k in pairs(cur.conditions or { }) do if type(k) == "string" then cv[k] = true end end
			for k in pairs(cur.multipliers or { }) do if type(k) == "string" then mv[k] = true end end
			cur = cur.parent
		end
		for _, name in ipairs(names) do
			local v = name:match("^Condition:(.*)$")
			if v then cv[v] = true end
			v = name:match("^Multiplier:(.*)$")
			if v then mv[v] = true end
		end
		for _, set in ipairs({ cv, mv }) do
			local vars = { }
			for k in pairs(set) do vars[#vars + 1] = k end
			sort(vars)
			for _, var in ipairs(vars) do
				if set == cv then
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

-- GetStat on the stores of the env's actors (player/enemy/minion).
for _, a in ipairs(__msb_no_queries and { } or { env.player, env.enemy, env.minion }) do
	if a and a.modDB and storeId[a.modDB] then
		local id = storeId[a.modDB]
		local stats = { "ManaReservedPercent", "LifeReservedPercent", "ManaUnreserved" }
		for k, v in pairs(a.output or { }) do
			if type(k) == "string" and type(v) == "number" then stats[#stats + 1] = k end
		end
		sort(stats)
		for ci = 1, nGlobalCfgs do
			for _, stat in ipairs(stats) do
				rec(id, "GetStat", ci, jstr(stat), pq(nil, a.modDB.GetStat, a.modDB, stat, cfgs[ci].cfg))
			end
		end
	end
end

---------------------------------------------------------------------------
-- Synthetic tag probes (`__msb_synthetic = true`): every distinct tag of PoB's parser cache
-- (modLib.parseModCache: ModCache.lua plus the lines parsed while loading) and of the skills'
-- stat maps / base mods, on a probe mod in a fresh ModDB child of the player / enemy / minion
-- modDB (so it sees this build's real actors, conditions, multipliers and outputs), queried with
-- every global cfg. Covers EvalMod branches the build's own mods do not reach.
---------------------------------------------------------------------------
local probeJson, synth = { }, { }
if __msb_synthetic then
	local probes = { }
	for _, t in ipairs(tags) do
		local tag = t[2]
		probes[#probes + 1] = { "BASE", { name = "MSBProbe", type = "BASE", value = 10, flags = 0, keywordFlags = 0, source = "Test", tag } }
		if tag.type == "Multiplier" or tag.type == "PerStat" or tag.type == "PercentStat" then
			probes[#probes + 1] = { "LIST", { name = "MSBProbeL", type = "LIST", value = { mod = { name = "MSBInner", type = "BASE", value = 3, flags = 0, keywordFlags = 0, source = "Test" } }, flags = 0, keywordFlags = 0, source = "Test", tag } }
			probes[#probes + 1] = { "LIST", { name = "MSBProbeL", type = "LIST", value = { key = "k", value = 2 }, flags = 0, keywordFlags = 0, source = "Test", tag } }
		end
	end
	for pi, p in ipairs(probes) do
		probeJson[pi] = '{"kind":' .. jstr(p[1]) .. ',"mod":' .. encMod(p[2]) .. "}"
	end
	-- sid 0: a ModDB made without a parent (`self.actor` is `{ }`)
	for _, a in ipairs({ env.player, env.enemy, env.minion or false, "none" }) do
		local base = type(a) == "table" and a.modDB or nil
		local sid = (a == "none") and 0 or (base and storeId[base])
		if sid then
			for pi, p in ipairs(probes) do
				local db = new("ModDB"):ModDB(base)
				db:AddMod(p[2])
				for ci = 1, nGlobalCfgs do
					local cfg = cfgs[ci].cfg
					local res
					if p[1] == "BASE" then
						res = pq(nil, db.Sum, db, "BASE", cfg, "MSBProbe")
					else
						res = pq("list", db.List, db, cfg, "MSBProbeL")
					end
					synth[#synth + 1] = "[" .. sid .. "," .. pi .. "," .. ci .. "," .. res .. "]"
				end
			end
		end
	end
end

return "{" .. concat({
	'"probes":[' .. concat(probeJson, ",") .. "]",
	'"synth":[' .. concat(synth, ",") .. "]",
	'"mods":[' .. concat(mods, ",") .. "]",
	'"stores":[' .. concat(storeJson, ",") .. "]",
	'"actors":[' .. concat(actorJson, ",") .. "]",
	'"items":[' .. concat(itemJson, ",") .. "]",
	'"cfgs":[' .. concat(cfgJson, ",") .. "]",
	'"results":[' .. concat(results, ",") .. "]",
}, ",") .. "}"
