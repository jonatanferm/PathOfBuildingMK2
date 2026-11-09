-- CS0 acceptance (crates/pob-oracle/tests/CS0.rs): PoB's side of the function probes of
-- CalcSetup.lua 1-474 (calc doc §11.1 row CS0).
--
-- Runs as the boot `pre` script of a patched PoB copy (pob_oracle::patcher with
-- corpus/CS0/stages.toml), before any of PoB loads. It is the probe runtime for that copy:
-- `__PROBE_FN(id, params, f)` wraps the six CS0 functions (initModDB,
-- buildModListForNodeList, wipeEnv, applyGemMods, applySocketMods, addBestSupport) and
-- returns every other probed function unwrapped; `__PROBE_CALL` is the identity; `__PROBE`,
-- `__PROV` and `__PROV_MOD` stay nil (the patched code tests them before use).
--
-- Each wrapper records the call's inputs before and its effects after the original runs, as
-- one JSON object per call (first `CAP[id]` calls per process). `__cs0.drive(build)` then calls
-- wipeEnv on fresh CALCULATOR envs with every accelerate shape. `__cs0.result()` returns
-- `{"records":[…],"counts":{…}}`.
--
-- Encodings (the MP7 corpus script's): mods as {"mod":{name,type,value,flags,keywordFlags,
-- source,"$seq":[tags]},"marks":{…}}; generic values in the dump shape ($seq/$num/$float);
-- jewel `data` tables keyed by tostring(key) with ModLists as {"$ModList":[mods]}.

local fmt, concat, sort = string.format, table.concat, table.sort
local floor, huge = math.floor, math.huge
local type, pairs, ipairs, tostring, rawget, getmetatable = type, pairs, ipairs, tostring, rawget, getmetatable

local CAP = {
	initModDB = 6,
	-- every call: the jewel funcs keep state across calls (getThreshold's LUA-ALIAS source,
	-- MP7), so the Rust side replays the calls in order
	buildModListForNodeList = 1000,
	wipeEnv = 4,
	applyGemMods = 80,
	-- the first 10 calls, then only calls with SocketProperty mods (up to SOCKET_CAP records)
	applySocketMods = 1000000,
	addBestSupport = 120,
}

local SOCKET_CAP = 60
local S = { records = { }, counts = { }, socketRecorded = 0 }
__cs0 = S

---------------------------------------------------------------------------
-- JSON
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
local function encData(t, depth)
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
			s = '{"$ModList":' .. encModList(v) .. "}"
		elseif type(v) == "table" then
			s = encData(v, depth + 1)
		else
			s = enc(v)
		end
		parts[#parts + 1] = jstr(type(k) == "number" and numKey(k) or tostring(k)) .. ":" .. s
	end
	return "{" .. concat(parts, ",") .. "}"
end
-- {"k": v, ...} from an ordered list of { key, json } pairs.
local function obj(fields)
	local parts = { }
	for _, f in ipairs(fields) do parts[#parts + 1] = jstr(f[1]) .. ":" .. f[2] end
	return "{" .. concat(parts, ",") .. "}"
end
local function arr(list)
	return "[" .. concat(list, ",") .. "]"
end
local function count(t)
	local n = 0
	if type(t) == "table" then for _ in pairs(t) do n = n + 1 end end
	return n
end

---------------------------------------------------------------------------
-- Probe runtime
---------------------------------------------------------------------------
local H = { }

function __PROBE_CALL(id, params, f) return f end

function __PROBE_FN(id, params, f)
	local h = H[id]
	if not h then return f end
	return function(...)
		local n = (S.counts[id] or 0) + 1
		S.counts[id] = n
		if n > CAP[id] then return f(...) end
		return h(f, ...)
	end
end

local function push(id, inJson, outJson, extra)
	local fields = { { "id", jstr(id) }, { "n", jnum(S.counts[id]) }, { "in", inJson }, { "out", outJson } }
	if extra then fields[#fields + 1] = { "extra", extra } end
	S.records[#S.records + 1] = obj(fields)
end

-- The parsed line a jewel func closure comes from (modLib.parseModCache: parseMod caches every
-- line and returns copies whose `value.func` is the cached closure), or the CalcSetup.lua:877
-- default func.
local jutil
do
	local ok, m = pcall(require, "jit.util")
	if ok and type(m) == "table" then jutil = m end
end
local funcIndex
local function scanFuncs()
	funcIndex = { }
	for line, entry in pairs(modLib.parseModCache) do
		local ml = type(entry) == "table" and entry[1]
		if type(ml) == "table" then
			for _, m in ipairs(ml) do
				local v = type(m) == "table" and m.value
				if type(v) == "table" and type(v.func) == "function" and not funcIndex[v.func] then
					funcIndex[v.func] = { line = line, name = m.name }
				end
			end
		end
	end
end
local function funcJson(f)
	if type(f) ~= "function" then return '{"none":true}' end
	local info = jutil and jutil.funcinfo(f)
	if type(info) == "table" and type(info.source) == "string" and info.source:find("CalcSetup%.lua$")
		and info.linedefined == 877 then
		return '{"default":true}'
	end
	if not funcIndex or not funcIndex[f] then scanFuncs() end
	local e = funcIndex[f]
	if not e then
		local src = type(info) == "table" and (tostring(info.source) .. ":" .. tostring(info.linedefined)) or "?"
		return obj({ { "unknown", jstr(src) } })
	end
	return obj({ { "line", jstr(e.line) }, { "name", jstr(tostring(e.name)) } })
end

local function nodeId(node)
	return type(node.id) == "number" and jnum(node.id) or jstr(tostring(node.id))
end
local function nodeJson(node)
	return obj({
		{ "id", nodeId(node) },
		{ "type", jstr(tostring(node.type)) },
		{ "tattoo", node.isTattoo and "true" or "false" },
		{ "conq", node.conqueredBy and "true" or "false" },
		{ "mods", node.modList and encModList(node.modList) or "null" },
		{ "ks", node.keystoneMod and encMod(node.keystoneMod) or "null" },
	})
end

---------------------------------------------------------------------------
-- initModDB(env, modDB)
---------------------------------------------------------------------------
H.initModDB = function(f, env, modDB)
	local inJson = obj({
		{ "buffs", enc(env.mode_buffs) },
		{ "combat", enc(env.mode_combat) },
		{ "effective", enc(env.mode_effective) },
		{ "preMods", jnum(count(modDB.mods)) },
		{ "pre", (function()
			local names = { }
			for name in pairs(modDB.mods) do names[#names + 1] = name end
			sort(names)
			local parts = { }
			for _, name in ipairs(names) do parts[#parts + 1] = jstr(name) .. ":" .. encModList(modDB.mods[name]) end
			return "{" .. concat(parts, ",") .. "}"
		end)() },
	})
	f(env, modDB)
	local names = { }
	for name in pairs(modDB.mods) do names[#names + 1] = name end
	sort(names)
	local mods = { }
	for _, name in ipairs(names) do mods[#mods + 1] = jstr(name) .. ":" .. encModList(modDB.mods[name]) end
	local outJson = obj({
		{ "mods", "{" .. concat(mods, ",") .. "}" },
		{ "conditions", encData(modDB.conditions) },
	})
	push("initModDB", inJson, outJson)
end

---------------------------------------------------------------------------
-- buildModListForNodeList(env, nodeList, finishJewels)
---------------------------------------------------------------------------
H.buildModListForNodeList = function(f, env, nodeList, finishJewels)
	local list, extra, allocIds = { }, { }, { }
	local listNodes, extraNodes = { }, { }
	for _, node in pairs(nodeList) do
		listNodes[#listNodes + 1] = node
		list[#list + 1] = nodeJson(node)
	end
	for _, node in pairs(env.extraRadiusNodeList) do
		extraNodes[#extraNodes + 1] = node
		extra[#extra + 1] = nodeJson(node)
	end
	for id in pairs(env.allocNodes) do allocIds[#allocIds + 1] = enc(id) end
	-- env.allocNodes starts as copyTable(env.spec.allocNodes, true) (CalcSetup.lua:770): the
	-- spec's pairs order is the copy's insertion order, which lets the Rust side rebuild the
	-- same LuaJIT table layout
	local specOrder = { }
	if env.spec and env.spec.allocNodes then
		for id in pairs(env.spec.allocNodes) do specOrder[#specOrder + 1] = enc(id) end
	end
	local rads = { }
	for i, rad in ipairs(env.radiusJewelList) do
		local nodes = { }
		for id, n in pairs(rad.nodes or { }) do
			nodes[#nodes + 1] = arr({ enc(id), jstr(tostring(n.type)) })
		end
		rads[i] = obj({
			{ "type", enc(rad.type) },
			{ "nodeId", enc(rad.nodeId) },
			{ "itemId", enc(rad.item and rad.item.id) },
			{ "func", funcJson(rad.func) },
			{ "nodes", arr(nodes) },
		})
	end
	local inJson = obj({
		{ "mode", jstr(tostring(env.mode)) },
		{ "finishJewels", finishJewels and "true" or "false" },
		{ "isAlloc", nodeList == env.allocNodes and "true" or "false" },
		{ "allocIds", arr(allocIds) },
		{ "specOrder", arr(specOrder) },
		{ "nodes", arr(list) },
		{ "extra", arr(extra) },
		{ "rads", arr(rads) },
	})
	local modList, explodeSources = f(env, nodeList, finishJewels)
	local ex = { }
	for i, e in ipairs(explodeSources) do
		ex[i] = type(e) == "table" and nodeId(e) or enc(e)
	end
	local per = { }
	for i, node in ipairs(listNodes) do
		local gs = { }
		for j, g in ipairs(node.grantedSkills or { }) do gs[j] = enc(g) end
		per[i] = obj({
			{ "final", env.mode == "MAIN" and node.finalModList and encModList(node.finalModList) or "null" },
			{ "granted", node.grantedSkills and arr(gs) or "null" },
		})
	end
	local perExtra = { }
	for i, node in ipairs(extraNodes) do
		perExtra[i] = obj({
			{ "final", env.mode == "MAIN" and node.finalModList and encModList(node.finalModList) or "null" },
		})
	end
	local radOut = { }
	for i, rad in ipairs(env.radiusJewelList) do
		local jrd = rad.item and rad.item.jewelRadiusData
		radOut[i] = obj({
			{ "data", encData(rad.data) },
			{ "jrd", jrd and (jrd[rad.nodeId] == rad.data and "true" or "false") or "null" },
		})
	end
	local outJson = obj({
		{ "modList", encModList(modList) },
		{ "explode", arr(ex) },
		{ "nodes", arr(per) },
		{ "extra", arr(perExtra) },
		{ "rads", arr(radOut) },
	})
	push("buildModListForNodeList", inJson, outJson)
	return modList, explodeSources
end

---------------------------------------------------------------------------
-- wipeEnv(env, accelerate)
---------------------------------------------------------------------------
local function wipeSizes(env)
	local minion = env.minion
	local function store(db)
		return arr({ jnum(count(db.mods)), jnum(count(db.conditions)), jnum(count(db.multipliers)) })
	end
	return obj({
		{ "modDB", store(env.modDB) },
		{ "enemyDB", store(env.enemyDB) },
		{ "minionDB", minion and store(minion.modDB) or "null" },
		{ "allocNodes", jnum(count(env.allocNodes)) },
		{ "grantedPassives", jnum(count(env.grantedPassives)) },
		{ "grantedSkillsNodes", jnum(count(env.grantedSkillsNodes)) },
		{ "itemModDB", store(env.itemModDB) },
		{ "radiusJewelList", jnum(count(env.radiusJewelList)) },
		{ "extraRadiusNodeList", jnum(count(env.extraRadiusNodeList)) },
		{ "itemList", jnum(count(env.player.itemList)) },
		{ "grantedSkillsItems", jnum(count(env.grantedSkillsItems)) },
		{ "flasks", jnum(count(env.flasks)) },
		{ "tinctures", jnum(count(env.tinctures)) },
		{ "aegisModList", env.aegisModList and jnum(count(env.aegisModList)) or "null" },
		{ "theIronMass", env.theIronMass and jnum(count(env.theIronMass)) or "null" },
		{ "weaponModList1", env.weaponModList1 and jnum(count(env.weaponModList1)) or "null" },
		{ "requirementsTableItems", jnum(count(env.requirementsTableItems)) },
		{ "requirementsTableGems", jnum(count(env.requirementsTableGems)) },
		{ "activeSkillList", jnum(count(env.player.activeSkillList)) },
		{ "auxSkillList", jnum(count(env.auxSkillList)) },
	})
end
H.wipeEnv = function(f, env, accelerate)
	local acc = { }
	for _, k in ipairs({ "nodeAlloc", "requirementsItems", "requirementsGems", "skills", "everything" }) do
		acc[#acc + 1] = { k, accelerate[k] and "true" or "false" }
	end
	local inJson = obj({ { "accelerate", obj(acc) }, { "sizes", wipeSizes(env) } })
	f(env, accelerate)
	push("wipeEnv", inJson, wipeSizes(env))
end

---------------------------------------------------------------------------
-- applyGemMods(effect, modList)
---------------------------------------------------------------------------
local effectKeys = { "level", "quality", "globalQuality", "itemQuality", "supportQuality", "req" }
local function effectJson(effect, rows)
	local keys, seen = { }, { }
	for _, k in ipairs(effectKeys) do keys[#keys + 1] = k seen[k] = true end
	for _, row in ipairs(rows) do
		local k = type(row.value) == "table" and row.value.key
		if type(k) == "string" and not seen[k] then keys[#keys + 1] = k seen[k] = true end
	end
	local fields = { }
	for _, k in ipairs(keys) do fields[#fields + 1] = { k, enc(effect[k]) } end
	return obj(fields)
end
H.applyGemMods = function(f, effect, modList)
	local rows = { }
	for i, row in ipairs(modList) do
		rows[i] = obj({ { "value", enc(row.value) }, { "mod", encMod(row.mod) } })
	end
	local gpi = effect.gemPropertyInfo
	local inJson = obj({
		{ "ge", jstr(tostring(effect.grantedEffect and effect.grantedEffect.id)) },
		{ "gem", effect.gemData and jstr(tostring(effect.gemData.id)) or "null" },
		{ "effect", effectJson(effect, modList) },
		{ "gpi", gpi and jnum(#gpi) or "null" },
		{ "rows", arr(rows) },
	})
	local pre = gpi and #gpi or 0
	f(effect, modList)
	local appended = { }
	for i = pre + 1, #(effect.gemPropertyInfo or { }) do
		local r = effect.gemPropertyInfo[i]
		local idx = 0
		for j, row in ipairs(modList) do if row == r then idx = j break end end
		appended[#appended + 1] = jnum(idx)
	end
	local outJson = obj({
		{ "effect", effectJson(effect, modList) },
		{ "gpi", effect.gemPropertyInfo and jnum(#effect.gemPropertyInfo) or "null" },
		{ "appended", arr(appended) },
	})
	push("applyGemMods", inJson, outJson)
end

---------------------------------------------------------------------------
-- applySocketMods(env, gem, groupCfg, socketNum, modSource)
---------------------------------------------------------------------------
H.applySocketMods = function(f, env, gem, groupCfg, socketNum, modSource)
	local cfgFields = { }
	local keys = { }
	for k, v in pairs(groupCfg) do
		if type(k) == "string" and (type(v) == "string" or type(v) == "number" or type(v) == "boolean") then
			keys[#keys + 1] = k
		end
	end
	sort(keys)
	for _, k in ipairs(keys) do cfgFields[#cfgFields + 1] = { k, enc(groupCfg[k]) } end
	local props = env.modDB.mods["SocketProperty"] or { }
	if S.counts.applySocketMods > 10 and (#props == 0 or S.socketRecorded >= SOCKET_CAP) then
		return f(env, gem, groupCfg, socketNum, modSource)
	end
	S.socketRecorded = S.socketRecorded + 1
	local inJson = obj({
		{ "gem", jstr(tostring(gem and gem.id)) },
		{ "groupCfg", obj(cfgFields) },
		{ "socketNum", enc(socketNum) },
		{ "modSource", enc(modSource) },
		{ "props", encModList(props) },
		{ "conditions", encData(env.modDB.conditions) },
		{ "multipliers", encData(env.modDB.multipliers) },
		{ "sameDB", env.player.modDB == env.modDB and "true" or "false" },
	})
	local db = env.player.modDB
	local added = { }
	local origAddMod = db.AddMod
	rawset(db, "AddMod", function(self, m)
		added[#added + 1] = encMod(m)
		return origAddMod(self, m)
	end)
	local ok, err = pcall(f, env, gem, groupCfg, socketNum, modSource)
	rawset(db, "AddMod", nil)
	if not ok then error(err, 0) end
	push("applySocketMods", inJson, obj({ { "added", arr(added) } }))
end

---------------------------------------------------------------------------
-- addBestSupport(supportEffect, appliedSupportList, mode)
---------------------------------------------------------------------------
local function supJson(e)
	return obj({
		{ "ge", jstr(tostring(e.grantedEffect and e.grantedEffect.id)) },
		{ "level", enc(e.level) },
		{ "quality", enc(e.quality) },
		{ "superseded", enc(e.superseded) },
	})
end
H.addBestSupport = function(f, supportEffect, appliedSupportList, mode)
	local before = { }
	local list = { }
	for i, e in ipairs(appliedSupportList) do
		before[i] = e
		list[i] = supJson(e)
	end
	local inJson = obj({
		{ "support", supJson(supportEffect) },
		{ "list", arr(list) },
		{ "mode", jstr(tostring(mode)) },
	})
	f(supportEffect, appliedSupportList, mode)
	local after = { }
	for i, e in ipairs(appliedSupportList) do
		local idx = -1
		if e == supportEffect then idx = 0 else
			for j, b in ipairs(before) do if b == e then idx = j break end end
		end
		after[i] = jnum(idx)
	end
	local sup = { }
	for i, b in ipairs(before) do sup[i] = enc(b.superseded) end
	push("addBestSupport", inJson, obj({
		{ "list", arr(after) },
		{ "supportSuperseded", enc(supportEffect.superseded) },
		{ "superseded", arr(sup) },
	}))
end

---------------------------------------------------------------------------
-- Driver: wipeEnv on fresh CALCULATOR envs, one per accelerate shape.
---------------------------------------------------------------------------
function S.drive(build)
	local calcs = require("Modules.Calcs")
	local shapes = {
		{ },
		{ nodeAlloc = true },
		{ requirementsItems = true },
		{ requirementsGems = true },
		{ skills = true },
		{ nodeAlloc = true, requirementsItems = true, requirementsGems = true, skills = true },
		{ everything = true },
	}
	CAP.wipeEnv = (S.counts.wipeEnv or 0) + #shapes
	for _, accel in ipairs(shapes) do
		local env = calcs.initEnv(build, "CALCULATOR")
		calcs.perform(env)
		wipeEnv(env, accel)
	end
end

function S.result()
	local c = { }
	for k, v in pairs(S.counts) do c[#c + 1] = { k, jnum(v) } end
	sort(c, function(a, b) return a[1] < b[1] end)
	return obj({ { "records", arr(S.records) }, { "counts", obj(c) } })
end
