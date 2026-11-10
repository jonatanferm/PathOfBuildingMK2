-- CS1 acceptance (crates/pob-oracle/tests/CS1.rs): PoB's side of the L4 check of initEnv
-- segment CS1 (Modules/CalcSetup.lua 488-799, calc doc §11.1 row CS1).
--
-- Appended to the probe prelude of `pob_oracle::runner::probe_prelude` (state_dump.lua +
-- probe.lua, provenance off), so the standard probe runtime writes the pob-state dumps of the
-- `initEnv:488` / `initEnv:800` line probes (corpus/CS1/stages.toml). This file wraps
-- `__PROBE`:
--
-- * it forwards an initEnv call's two probes to the probe runtime only when the call is
--   selected (the first CAP calls of each (mode, path) shape during the build load, and every
--   call `__cs1.drive` makes), so the runtime's hit numbers `#k` count the selected calls;
-- * at a selected `initEnv:488` it writes `<outDir>/model/model#k.json`: a pob-state document
--   (S.encodeDoc, no env) whose `locals.model` holds the part of the build model segment CS1
--   reads (the Rust test rebuilds a `Build` from it; BM-A's loader is not ported): character
--   level, the config/calcs inputs, the config and party mod lists, the spec's counters and
--   allocated nodes (id, type, name, isTattoo, overrideType, modList, keystoneMod, in
--   `pairs` order), the class base stats, the keystone map and the call's `override`.
--
-- `__cs1.drive(build)` then calls initEnv with overrides (addNodes / removeNodes / conditions),
-- specEnv shapes (cached DBs with and without an env and accelerate flags) and config / calcs
-- input variants (bandits, pantheons, resistance penalty, enemy level, buff modes), restoring
-- the build afterwards.

local S = __state_dump
local cfg = __probe_cfg
local type, pairs, ipairs, tostring, next = type, pairs, ipairs, tostring, next
local sort = table.sort

local CAP = 2
local C = { forwarded = 0, shapes = { }, driving = false, current = false, counts = { } }
__cs1 = C

local function writeFile(path, s)
	local f, err = io.open(path, "wb")
	if not f then error("cs1: cannot write " .. tostring(path) .. ": " .. tostring(err)) end
	f:write(s)
	f:close()
end

local function nodeRec(node)
	return {
		id = node.id,
		type = node.type,
		name = node.name,
		isTattoo = node.isTattoo and true or false,
		overrideType = node.overrideType,
		modList = node.modList,
		keystoneMod = node.keystoneMod,
	}
end

local function copyPlain(t)
	local out = { }
	if type(t) ~= "table" then return out end
	for k, v in pairs(t) do
		if type(v) ~= "table" and type(v) ~= "function" then out[k] = v end
	end
	return out
end

local function model(build, override)
	local spec = build.spec
	local tree = spec.tree
	local alloc = { }
	for _, node in pairs(spec.allocNodes) do alloc[#alloc + 1] = nodeRec(node) end
	local ks = { }
	for name, node in pairs(tree.keystoneMap or { }) do
		ks[#ks + 1] = { name = name, id = node.id, modList = node.modList, type = node.type }
	end
	sort(ks, function(a, b) return a.name < b.name end)
	local classId = spec.curClassId
	local function stats(t)
		if type(t) ~= "table" then return nil end
		return { base_str = t.base_str, base_dex = t.base_dex, base_int = t.base_int }
	end
	local m = {
		characterLevel = build.characterLevel,
		enemyLevel = build.configTab.enemyLevel,
		input = copyPlain(build.configTab.input),
		placeholder = copyPlain(build.configTab.placeholder),
		calcsInput = copyPlain(build.calcsTab.input),
		configModList = build.configTab.modList,
		configEnemyModList = build.configTab.enemyModList,
		partyEnemyModList = build.partyTab.enemyModList,
		treeVersion = spec.treeVersion,
		curClassId = classId,
		allocatedNotableCount = spec.allocatedNotableCount,
		allocatedKeystoneCount = spec.allocatedKeystoneCount,
		allocatedMasteryCount = spec.allocatedMasteryCount,
		allocatedMasteryTypeCount = spec.allocatedMasteryTypeCount,
		allocatedMasteryTypes = copyPlain(spec.allocatedMasteryTypes),
		allocatedTattooTypes = copyPlain(spec.allocatedTattooTypes),
		allocNodes = alloc,
		classStats = stats(tree.classes and tree.classes[classId]),
		characterData = tree.characterData and { stats = stats(tree.characterData[classId]) } or nil,
		keystones = ks,
	}
	local ov = { }
	if type(override) == "table" then
		if override.spec then ov.spec = true end
		if override.addNodes then
			ov.addNodes = { }
			for node in pairs(override.addNodes) do ov.addNodes[#ov.addNodes + 1] = nodeRec(node) end
		end
		if override.removeNodes then
			ov.removeNodes = { }
			for node in pairs(override.removeNodes) do ov.removeNodes[#ov.removeNodes + 1] = node.id end
		end
		if override.conditions then
			ov.conditions = { }
			for i, c in ipairs(override.conditions) do ov.conditions[i] = c end
		end
		-- set by CalcSetup.lua:1424-1430 before the restart (CS2); CS1 does not read it
		ov.extraJewelFuncs = override.extraJewelFuncs
		for k in pairs(override) do
			if k ~= "spec" and k ~= "addNodes" and k ~= "removeNodes" and k ~= "conditions"
				and k ~= "extraJewelFuncs" then
				ov.other = (ov.other or "") .. tostring(k) .. " "
			end
		end
	end
	m.override = ov
	return m
end

local function shapeOf(locals)
	local mode = locals.mode
	local specEnv = locals.specEnv
	local path = "fresh"
	if type(specEnv) == "table" then
		if specEnv.env then path = "env" elseif specEnv.cachedPlayerDB then path = "cached" end
		if specEnv.accelerate then
			local a = { }
			for k, v in pairs(specEnv.accelerate) do if v then a[#a + 1] = k end end
			sort(a)
			path = path .. "[" .. table.concat(a, ",") .. "]"
		end
	end
	local ov = locals.override
	if type(ov) == "table" and next(ov) ~= nil then path = path .. "+override" end
	return tostring(mode) .. ":" .. path
end

local orig = __PROBE
function __PROBE(id, locals)
	if id == "initEnv:488" then
		local shape = shapeOf(locals)
		C.counts[shape] = (C.counts[shape] or 0) + 1
		local take = C.driving or C.counts[shape] <= CAP
		C.current = take
		if take then
			C.forwarded = C.forwarded + 1
			C.shapes[C.forwarded] = shape
			-- the model is encoded without the GlobalCache (encodeDoc dumps it otherwise)
			local gc = rawget(_G, "GlobalCache")
			rawset(_G, "GlobalCache", nil)
			local ok, json = pcall(S.encodeDoc, { build = cfg.build, hit = "cs1model#" .. C.forwarded,
				probe = "cs1model", n = C.forwarded, kind = "line", stack = { },
				locals = { model = model(locals.build, locals.override) } })
			rawset(_G, "GlobalCache", gc)
			if not ok then error(json, 0) end
			writeFile(cfg.outDir .. "/model/model#" .. C.forwarded .. ".json", json)
		else
			return
		end
	elseif id == "initEnv:800" then
		if not C.current then return end
	end
	return orig(id, locals)
end

-- The first node (lowest id) of `nodes` (id → node) with `pred`.
local function firstNode(nodes, pred)
	local best
	for id, node in pairs(nodes) do
		if type(id) == "number" and pred(node) and (not best or id < best.id) then best = node end
	end
	return best
end

function C.drive(build)
	local calcs = require("Modules.Calcs")
	C.driving = true
	local spec = build.spec
	local alloc = spec.allocNodes
	local function free(t)
		return function(n)
			return n.type == t and not alloc[n.id] and not n.ascendancyName and n.modList ~= nil
		end
	end
	local function held(t)
		return function(n) return n.type == t end
	end
	-- overrides: one added node per call (a node-keyed set's pairs order is PoB's address order)
	for _, t in ipairs({ "Notable", "Keystone", "Mastery", "Normal" }) do
		local n = firstNode(spec.nodes, free(t))
		if n then calcs.initEnv(build, "CALCULATOR", { addNodes = { [n] = true } }) end
	end
	local rem = { }
	for _, t in ipairs({ "Mastery", "Notable", "Keystone", "Normal" }) do
		local n = firstNode(alloc, held(t))
		if n then
			rem[n] = true
			calcs.initEnv(build, "CALCULATOR", { removeNodes = { [n] = true } })
		end
	end
	for id, n in pairs(alloc) do
		if n.isTattoo and n.overrideType then
			calcs.initEnv(build, "CALCULATOR", { removeNodes = { [n] = true } })
			break
		end
	end
	if next(rem) then
		local add = firstNode(spec.nodes, free("Notable"))
		calcs.initEnv(build, "CALCULATOR", { addNodes = add and { [add] = true } or nil, removeNodes = rem,
			conditions = { "CS1ProbeA", "CS1ProbeB" } })
	end
	calcs.initEnv(build, "MAIN", { conditions = { "CS1Probe" } })
	-- specEnv shapes
	local env, p, e, m = calcs.initEnv(build, "CALCULATOR")
	calcs.perform(env)
	calcs.initEnv(build, "CALCULATOR", nil, { cachedPlayerDB = p, cachedEnemyDB = e, cachedMinionDB = m })
	calcs.initEnv(build, "CALCULATOR", nil, { cachedPlayerDB = p, cachedEnemyDB = e, cachedMinionDB = m,
		env = env, accelerate = { nodeAlloc = true, requirementsItems = true, requirementsGems = true, skills = true } })
	calcs.perform(env)
	calcs.initEnv(build, "CALCULATOR", { conditions = { "CS1Probe" } }, { env = env, accelerate = { } })
	-- config and calcs input variants
	local input = build.configTab.input
	local calcsInput = build.calcsTab.input
	local saved = { bandit = input.bandit, major = input.pantheonMajorGod, minor = input.pantheonMinorGod,
		pen = input.resistancePenalty, level = build.configTab.enemyLevel, buff = calcsInput.misc_buffMode,
		charLevel = build.characterLevel }
	for _, b in ipairs({ "Alira", "Kraityn", "Oak", "None" }) do
		input.bandit = b
		calcs.initEnv(build, "MAIN")
	end
	input.bandit = saved.bandit
	local gods = { }
	for name in pairs(build.data.pantheons) do gods[#gods + 1] = name end
	sort(gods)
	local majors, minors = { }, { }
	for _, g in ipairs(gods) do
		if build.data.pantheons[g].isMajorGod then majors[#majors + 1] = g else minors[#minors + 1] = g end
	end
	for i = 1, math.max(#majors, #minors) do
		input.pantheonMajorGod = majors[i] or "None"
		input.pantheonMinorGod = minors[i] or "None"
		calcs.initEnv(build, "MAIN")
	end
	input.pantheonMajorGod, input.pantheonMinorGod = saved.major, saved.minor
	for _, pen in ipairs({ 0, -30 }) do
		input.resistancePenalty = pen
		calcs.initEnv(build, "MAIN")
	end
	input.resistancePenalty = saved.pen
	build.configTab.enemyLevel = 70
	calcs.initEnv(build, "MAIN")
	build.configTab.enemyLevel = saved.level
	build.characterLevel = 100
	calcs.initEnv(build, "CALCULATOR")
	build.characterLevel = saved.charLevel
	for _, mode in ipairs({ "EFFECTIVE", "COMBAT", "BUFFED", "UNBUFFED" }) do
		calcsInput.misc_buffMode = mode
		calcs.initEnv(build, "CALCS")
	end
	calcsInput.misc_buffMode = saved.buff
	C.driving = false
end

function C.result()
	return S.json({ forwarded = C.forwarded, shapes = S.array(C.shapes), counts = C.counts })
end
