-- pob-oracle: dumps of Path of Building's calc state as JSON.
--
-- Run once after PoB has booted (BOOT_SCRIPT) and before the build loads.
-- Nothing in PoB's sources is edited: `calcs.initEnv` is wrapped from here
-- so the main env can be snapshotted the moment it is initialised, before
-- `calcs.perform` adds to it. See crates/pob-oracle/README.md for the JSON
-- conventions (they match pob_oracle::legacy_modifier_serde for mods and values).

local calcs = require("Modules.Calcs")
local fmt, concat, sort = string.format, table.concat, table.sort
local type, pairs, ipairs, tostring, tonumber = type, pairs, ipairs, tostring, tonumber
local getmetatable, rawget, next = getmetatable, rawget, next
local floor, huge = math.floor, math.huge

-- Keep stdout clean for JSON: PoB's console output goes to stderr.
function print(...)
	local n = select("#", ...)
	local parts = { }
	for i = 1, n do parts[i] = tostring((select(i, ...))) end
	io.stderr:write(concat(parts, "\t"), "\n")
end
function ConPrintf(f, ...)
	local ok, s = pcall(fmt, f, ...)
	io.stderr:write(ok and s or tostring(f), "\n")
end

---------------------------------------------------------------------------
-- JSON encoding
---------------------------------------------------------------------------

local escapes = { ['"'] = '\\"', ['\\'] = '\\\\', ['\n'] = '\\n', ['\t'] = '\\t', ['\r'] = '\\r' }
local function jstr(s)
	return '"' .. s:gsub('[%c"\\]', function(c)
		return escapes[c] or fmt("\\u%04x", c:byte())
	end) .. '"'
end

-- Shortest decimal that reads back to the same double.
local function jnum(v)
	if v ~= v then return '{"$num":"nan"}' end
	if v == huge then return '{"$num":"inf"}' end
	if v == -huge then return '{"$num":"-inf"}' end
	if v == floor(v) and v > -1e15 and v < 1e15 then return fmt("%d", v) end
	for _, p in ipairs({ "%.15g", "%.16g" }) do
		local s = fmt(p, v)
		if tonumber(s) == v then return s end
	end
	return fmt("%.17g", v)
end

local modTypes = { BASE = true, INC = true, MORE = true, FLAG = true, OVERRIDE = true, LIST = true,
	MAX = true, MIN = true, Flag = true, CHANCE = true, DUMMY = true }

local function isMod(t)
	return type(rawget(t, "name")) == "string" and modTypes[rawget(t, "type")] ~= nil
end

local function keyString(k)
	local tk = type(k)
	if tk == "string" then return k end
	if tk == "number" then return (jnum(k):gsub('^{"%$num":"(.*)"}$', "%1")) end
	if tk == "boolean" then return tostring(k) end
	return nil
end

local encode

-- A reference instead of the table, for objects that are not data: PoB
-- class instances, active skills, gem / granted-effect records.
local function refFor(t)
	local mt = getmetatable(t)
	if mt then
		local ok, cls = pcall(function() return t._className end)
		if ok and cls == "Item" then return '{"$item":' .. jstr(tostring(t.name)) .. '}' end
		if ok and type(cls) == "string" then return '{"$object":' .. jstr(cls) .. '}' end
	end
	if rawget(t, "activeEffect") and rawget(t, "skillFlags") then
		local ge = t.activeEffect.grantedEffect
		return '{"$skill":' .. jstr(tostring(ge and ge.id or "?")) .. '}'
	end
	if rawget(t, "grantedEffect") and rawget(t, "grantedEffectId") then
		return '{"$gem":' .. jstr(tostring(t.name)) .. '}'
	end
	if rawget(t, "nameSpec") and (rawget(t, "gemData") or rawget(t, "gemId") or rawget(t, "skillId")) then
		return '{"$gemInstance":' .. jstr(tostring(t.nameSpec)) .. '}'
	end
	if type(rawget(t, "id")) == "string" and type(rawget(t, "levels")) == "table"
		and (rawget(t, "stats") or rawget(t, "skillTypes") or rawget(t, "support") ~= nil) then
		return '{"$grantedEffect":' .. jstr(t.id) .. '}'
	end
	return nil
end

local function encodeMod(m, buf, depth, seen, maxDepth)
	buf[#buf + 1] = '{"name":' .. jstr(m.name) .. ',"type":' .. jstr(m.type) .. ',"value":'
	encode(m.value, buf, depth + 1, seen, maxDepth)
	buf[#buf + 1] = ',"flags":' .. jnum(m.flags or 0) .. ',"keywordFlags":' .. jnum(m.keywordFlags or 0)
	if m.source ~= nil then buf[#buf + 1] = ',"source":' .. jstr(tostring(m.source)) end
	buf[#buf + 1] = ',"tags":['
	local n = 0
	for i = 1, #m do
		if m[i] then
			if n > 0 then buf[#buf + 1] = "," end
			encode(m[i], buf, depth + 1, seen, maxDepth)
			n = n + 1
		end
	end
	buf[#buf + 1] = "]"
	-- Any further fields (replaced, converted, ...), sorted.
	local extra = { }
	for k, v in pairs(m) do
		if type(k) == "string" and k ~= "name" and k ~= "type" and k ~= "value" and k ~= "flags"
			and k ~= "keywordFlags" and k ~= "source" and type(v) ~= "function" then
			extra[#extra + 1] = k
		end
	end
	sort(extra)
	for _, k in ipairs(extra) do
		buf[#buf + 1] = "," .. jstr(k) .. ":"
		encode(m[k], buf, depth + 1, seen, maxDepth)
	end
	buf[#buf + 1] = "}"
end

-- Generic value encoder. Sequences become arrays; other tables objects with
-- sorted keys (numeric keys as strings); functions / userdata are dropped.
encode = function(v, buf, depth, seen, maxDepth)
	local tv = type(v)
	if tv == "nil" then buf[#buf + 1] = "null"
	elseif tv == "boolean" then buf[#buf + 1] = v and "true" or "false"
	elseif tv == "number" then buf[#buf + 1] = jnum(v)
	elseif tv == "string" then buf[#buf + 1] = jstr(v)
	elseif tv == "table" then
		if seen[v] then buf[#buf + 1] = '{"$cycle":true}' return end
		local ref = refFor(v)
		if ref then buf[#buf + 1] = ref return end
		if depth >= maxDepth then buf[#buf + 1] = '{"$depth":true}' return end
		seen[v] = true
		if isMod(v) then
			encodeMod(v, buf, depth, seen, maxDepth)
			seen[v] = nil
			return
		end
		local n = #v
		local count = 0
		for k, x in pairs(v) do
			if type(x) ~= "function" and type(x) ~= "userdata" then count = count + 1 end
		end
		local isArray = n > 0 and count == n
		if isArray then
			for i = 1, n do if v[i] == nil then isArray = false break end end
		end
		if isArray then
			buf[#buf + 1] = "["
			for i = 1, n do
				if i > 1 then buf[#buf + 1] = "," end
				encode(v[i], buf, depth + 1, seen, maxDepth)
			end
			buf[#buf + 1] = "]"
		else
			local keys, map = { }, { }
			for k, x in pairs(v) do
				local tx = type(x)
				if tx ~= "function" and tx ~= "userdata" and tx ~= "thread" then
					local ks = keyString(k)
					if ks and map[ks] == nil then
						keys[#keys + 1] = ks
						map[ks] = k
					end
				end
			end
			sort(keys)
			buf[#buf + 1] = "{"
			for i, ks in ipairs(keys) do
				if i > 1 then buf[#buf + 1] = "," end
				buf[#buf + 1] = jstr(ks) .. ":"
				encode(v[map[ks]], buf, depth + 1, seen, maxDepth)
			end
			buf[#buf + 1] = "}"
		end
		seen[v] = nil
	else
		buf[#buf + 1] = "null"
	end
end

local function toJson(v, maxDepth)
	local buf = { }
	encode(v, buf, 0, { }, maxDepth or 24)
	return concat(buf)
end
__oracle_json = toJson

-- A JSON object from an ordered list of { key, already-encoded JSON }.
local function obj(fields)
	local parts = { }
	for _, f in ipairs(fields) do
		if f[2] ~= nil then parts[#parts + 1] = jstr(f[1]) .. ":" .. f[2] end
	end
	return "{" .. concat(parts, ",") .. "}"
end

local function arr(list)
	return "[" .. concat(list, ",") .. "]"
end

local function jopt(v, maxDepth)
	if v == nil then return "null" end
	return toJson(v, maxDepth)
end

---------------------------------------------------------------------------
-- Stage dumps
---------------------------------------------------------------------------

-- Every mod of a ModDB (names sorted, PoB's order within a name) or a
-- ModList (its order), plus conditions / multipliers and the parent chain.
local function dumpModStore(store, depth)
	depth = depth or 0
	if not store then return "null" end
	local mods = { }
	if store.mods then
		local names = { }
		for name in pairs(store.mods) do names[#names + 1] = name end
		sort(names)
		for _, name in ipairs(names) do
			for _, m in ipairs(store.mods[name]) do
				mods[#mods + 1] = toJson(m)
			end
		end
	else
		for _, m in ipairs(store) do
			mods[#mods + 1] = toJson(m)
		end
	end
	return obj({
		{ "class", jstr(store._className or "?") },
		{ "mods", arr(mods) },
		{ "conditions", toJson(store.conditions or { }) },
		{ "multipliers", toJson(store.multipliers or { }) },
		{ "parent", (store.parent and depth < 4) and dumpModStore(store.parent, depth + 1) or "null" },
	})
end

local function sortedTrueKeys(t)
	local list = { }
	for k, v in pairs(t or { }) do
		if v and type(k) == "number" then list[#list + 1] = k end
	end
	sort(list)
	local out = { }
	for i, k in ipairs(list) do out[i] = jnum(k) end
	return arr(out)
end

local skillTypeName = { }
for name, id in pairs(SkillType or { }) do skillTypeName[id] = name end

local function skillTypeNames(t)
	local list = { }
	for k, v in pairs(t or { }) do
		if v and type(k) == "number" then list[#list + 1] = k end
	end
	sort(list)
	local out = { }
	for i, k in ipairs(list) do out[i] = jstr(skillTypeName[k] or tostring(k)) end
	return arr(out)
end

-- skillCfg with its object references reduced to names / ids.
local function dumpCfg(cfg)
	if not cfg then return "null" end
	local fields = { }
	local keys = { }
	for k in pairs(cfg) do if type(k) == "string" then keys[#keys + 1] = k end end
	sort(keys)
	for _, k in ipairs(keys) do
		local v = cfg[k]
		local enc
		if k == "skillGem" then
			enc = type(v) == "table" and jstr(tostring(v.name)) or jopt(v)
		elseif k == "skillGrantedEffect" then
			enc = type(v) == "table" and jstr(tostring(v.id)) or jopt(v)
		elseif k == "skillTypes" then
			enc = sortedTrueKeys(v)
		elseif type(v) ~= "function" then
			enc = toJson(v, 4)
		end
		if enc then fields[#fields + 1] = { k, enc } end
	end
	return obj(fields)
end

local function socketGroupIndex(group)
	if not group then return nil end
	for i, g in ipairs(build.skillsTab.socketGroupList) do
		if g == group then return i end
	end
	return nil
end

local function dumpEffects(skill)
	local list = { }
	for _, eff in ipairs(skill.effectList or { }) do
		local ge = eff.grantedEffect
		list[#list + 1] = obj({
			{ "id", jstr(tostring(ge and ge.id or "?")) },
			{ "name", ge and ge.name and jstr(ge.name) or "null" },
			{ "support", (ge and ge.support) and "true" or "false" },
			{ "level", jopt(eff.level) },
			{ "quality", jopt(eff.quality) },
			{ "qualityId", jopt(eff.qualityId) },
		})
	end
	return arr(list)
end

local function skillId(skill)
	local ge = skill.activeEffect and skill.activeEffect.grantedEffect
	return ge and ge.id or "?"
end

local function skillName(skill)
	local ge = skill.activeEffect and skill.activeEffect.grantedEffect
	return ge and ge.name or "?"
end

local function dumpSkill(env, i, skill)
	local group = skill.socketGroup
	return obj({
		{ "index", jnum(i) },
		{ "id", jstr(skillId(skill)) },
		{ "name", jstr(skillName(skill)) },
		{ "isMain", (skill == env.player.mainSkill) and "true" or "false" },
		{ "socketGroup", jopt(socketGroupIndex(group)) },
		{ "slot", jopt(group and group.slot) },
		{ "includeInFullDPS", (group and group.includeInFullDPS) and "true" or "false" },
		{ "skillPart", jopt(skill.skillPart) },
		{ "skillPartName", jopt(skill.skillPartName) },
		{ "minion", skill.minion and jopt(skill.minion.minionData and skill.minion.minionData.name) or "null" },
		{ "effects", dumpEffects(skill) },
		{ "skillTypes", sortedTrueKeys(skill.skillTypes) },
		{ "skillTypeNames", skillTypeNames(skill.skillTypes) },
		{ "skillFlags", toJson(skill.skillFlags or { }, 4) },
		{ "skillData", toJson(skill.skillData or { }, 4) },
		{ "skillCfg", dumpCfg(skill.skillCfg) },
		{ "weapon1Cfg", dumpCfg(skill.weapon1Cfg) },
		{ "weapon2Cfg", dumpCfg(skill.weapon2Cfg) },
		{ "skillModList", dumpModStore(skill.skillModList) },
	})
end

-- The env right after calcs.initEnv: actor mod DBs and active skills.
local function dumpInitEnv(env)
	local skills = { }
	local mainIndex
	for i, skill in ipairs(env.player.activeSkillList) do
		skills[#skills + 1] = dumpSkill(env, i, skill)
		if skill == env.player.mainSkill then mainIndex = i end
	end
	return obj({
		{ "mode", jstr(tostring(env.mode)) },
		{ "mainSkillIndex", jopt(mainIndex) },
		{ "player", obj({ { "level", jopt(env.player.level) }, { "modDB", dumpModStore(env.player.modDB) } }) },
		{ "enemy", obj({ { "level", jopt(env.enemy and env.enemy.level) }, { "modDB", dumpModStore(env.enemyDB) } }) },
		{ "minion", env.minion and obj({ { "level", jopt(env.minion.level) }, { "modDB", dumpModStore(env.minion.modDB) } }) or "null" },
		{ "itemModDB", dumpModStore(env.itemModDB) },
		{ "activeSkills", arr(skills) },
	})
end

-- Wrap calcs.initEnv: while armed, the first MAIN-mode env built without a
-- spec env (calcs.buildOutput's own) is dumped as soon as it returns.
local origInitEnv = calcs.initEnv
__oracle = { armed = false }
calcs.initEnv = function(b, mode, override, specEnv)
	local env, p, e, m = origInitEnv(b, mode, override, specEnv)
	if __oracle.armed and mode == "MAIN" and not specEnv then
		__oracle.armed = false
		__oracle.initEnv = env
		__oracle.initEnvJson = dumpInitEnv(env)
	end
	return env, p, e, m
end

---------------------------------------------------------------------------
-- Entry points
---------------------------------------------------------------------------

local function mainEnv()
	local b = __mainObject__.main.modes["BUILD"]
	return b.calcsTab and b.calcsTab.mainEnv
end

local function meta()
	return {
		{ "build", jstr(tostring(__pob_name or "")) },
		{ "pobVersion", jopt(launch and launch.versionNumber) },
	}
end

local function mainSkillInfo(env)
	local s = env.player.mainSkill
	if not s then return "null" end
	local idx
	for i, x in ipairs(env.player.activeSkillList) do if x == s then idx = i end end
	return obj({ { "index", jopt(idx) }, { "id", jstr(skillId(s)) }, { "name", jstr(skillName(s)) },
		{ "skillPartName", jopt(s.skillPartName) } })
end

-- The main env's complete player (and minion) output.
function __oracle_outputs()
	local env = mainEnv()
	if not env then error("no main env: build did not calculate") end
	local fields = meta()
	fields[#fields + 1] = { "format", jstr("pob-oracle/outputs/1") }
	fields[#fields + 1] = { "mainSkill", mainSkillInfo(env) }
	fields[#fields + 1] = { "player", toJson(env.player.output) }
	fields[#fields + 1] = { "minion", env.minion and env.minion.output and toJson(env.minion.output) or "null" }
	return obj(fields)
end

-- Same count logic as Calcs.lua's local getActiveSkillCount.
local function activeSkillCount(activeSkill)
	if not activeSkill.socketGroup then
		return 1, true
	elseif activeSkill.socketGroup.groupCount then
		return activeSkill.socketGroup.groupCount, true
	else
		for _, gemData in pairs(activeSkill.socketGroup.gemList) do
			if gemData.gemData then
				if gemData.gemData.vaalGem then
					if activeSkill.activeEffect.grantedEffect == gemData.gemData.grantedEffectList[1] then
						return gemData.count or 1, gemData.enableGlobal1 == true
					elseif activeSkill.activeEffect.grantedEffect == gemData.gemData.grantedEffectList[2] then
						return gemData.count or 1, gemData.enableGlobal2 == true
					end
				else
					if (activeSkill.activeEffect.grantedEffect == gemData.gemData.grantedEffect and not gemData.gemData.grantedEffect.support) or (activeSkill.activeEffect.grantedEffect == gemData.gemData.secondaryGrantedEffect) then
						return gemData.count or 1, true
					end
				end
			end
		end
	end
	return 1, true
end

-- Final output of each socket-group skill as the main skill, computed the
-- way calcs.calcFullDPS does (CALCULATOR env, mainSkill set, perform with
-- skipEHP) but from a fresh initEnv per skill.
local function skillOutputs(build)
	local list = { }
	local base = origInitEnv(build, "CALCULATOR")
	local n = #base.player.activeSkillList
	for i = 1, n do
		local s0 = base.player.activeSkillList[i]
		if s0.socketGroup then
			local env = origInitEnv(build, "CALCULATOR")
			local skill = env.player.activeSkillList[i]
			local count, enabled = activeSkillCount(skill)
			local ok, err = pcall(function()
				env.player.mainSkill = skill
				calcs.perform(env, true)
			end)
			local fields = {
				{ "index", jnum(i) },
				{ "id", jstr(skillId(skill)) },
				{ "name", jstr(skillName(skill)) },
				{ "socketGroup", jopt(socketGroupIndex(skill.socketGroup)) },
				{ "includeInFullDPS", skill.socketGroup.includeInFullDPS and "true" or "false" },
				{ "count", jopt(count) },
				{ "enabled", enabled and "true" or "false" },
				{ "skillPartName", jopt(skill.skillPartName) },
				{ "error", ok and "null" or jstr(tostring(err)) },
			}
			if ok then
				fields[#fields + 1] = { "output", toJson(env.player.output) }
				fields[#fields + 1] = { "minionOutput", (env.minion and env.minion.output) and toJson(env.minion.output) or "null" }
				if skill.mirage then
					fields[#fields + 1] = { "mirage", obj({
						{ "name", jopt(skill.mirage.name) },
						{ "count", jopt(skill.mirage.count) },
						{ "output", jopt(skill.mirage.output) },
					}) }
				end
			end
			list[#list + 1] = obj(fields)
		end
	end
	return arr(list)
end

-- Stage dumps: the main env right after initEnv, then per-skill outputs.
function __oracle_stages()
	local b = __mainObject__.main.modes["BUILD"]
	__oracle.armed = true
	__oracle.initEnvJson = nil
	b.calcsTab:BuildOutput()
	__oracle.armed = false
	if not __oracle.initEnvJson then error("calcs.initEnv(MAIN) was not called by BuildOutput") end
	local fields = meta()
	fields[#fields + 1] = { "format", jstr("pob-oracle/stages/1") }
	fields[#fields + 1] = { "initEnv", __oracle.initEnvJson }
	fields[#fields + 1] = { "mainEnvIsCaptured", (b.calcsTab.mainEnv == __oracle.initEnv) and "true" or "false" }
	fields[#fields + 1] = { "skillOutputs", skillOutputs(b) }
	return obj(fields)
end
