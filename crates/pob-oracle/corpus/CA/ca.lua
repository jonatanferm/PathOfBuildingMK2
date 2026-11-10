-- CA acceptance (crates/pob-oracle/tests/CA.rs): PoB's values after a build load, as JSON.
-- Runs in the oracle process after the load script (oracle.lua's `__oracle_json` encoder).
-- Owned by CA.
--
-- * main / calcs: calcsTab.mainOutput / calcsOutput (the L6 comparison) and the minion outputs.
-- * mainUsed: calcsTab.mainEnv's used-tracking tables (Calcs.lua 448-468) as { var = #mods },
--   skillsUsed as a sorted name list; calcsBreakdown: the CALCS env's SkillBuffs / SkillDebuffs
--   row counts.
-- * costs: GlobalCache.cachedData.MAIN[uuid] as the cost warnings read it (Calcs.lua 408/413/
--   426): the cached env's cost outputs and its main skill's `<pool>ReservedPercent`.
-- * misc: the misc calculator (CalcsTab.lua:470-471, Build.lua:1272): its base output, then
--   `calcFunc({})` and `calcFunc({}, false)`, headline keys only.
local b = __mainObject__.main.modes["BUILD"]
local ct = b.calcsTab

local function counts(t)
	local r = { }
	for k, v in pairs(t or { }) do
		r[k] = #v
	end
	return r
end

local function sortedKeys(t)
	local r = { }
	for k in pairs(t or { }) do
		r[#r + 1] = k
	end
	table.sort(r)
	return r
end

local usedNames = { "conditionsUsed", "enemyConditionsUsed", "minionConditionsUsed", "multipliersUsed",
	"enemyMultipliersUsed", "perStatsUsed", "enemyPerStatsUsed", "tagTypesUsed", "modsUsed" }

local function used(env)
	local r = { skillsUsed = sortedKeys(env.skillsUsed) }
	for _, n in ipairs(usedNames) do
		r[n] = counts(env[n])
	end
	return r
end

local costKeys = { "LifeCost", "ManaCost", "RageCost", "ESCost", "LifePercentCost", "ManaPercentCost" }
local resKeys = { "LifeReservedPercent", "ManaReservedPercent", "RageReservedPercent", "EnergyShieldReservedPercent" }
local costs = { }
for uuid, e in pairs(GlobalCache.cachedData.MAIN) do
	local c = { output = { }, skillData = { }, hasMainSkill = e.Env.player.mainSkill ~= nil }
	for _, k in ipairs(costKeys) do
		c.output[k] = e.Env.player.output[k]
	end
	local ms = e.Env.player.mainSkill
	if ms then
		for _, k in ipairs(resKeys) do
			c.skillData[k] = ms.skillData[k]
		end
	end
	costs[uuid] = c
end

local function rowCount(bd, key)
	return bd and bd[key] and bd[key].modList and #bd[key].modList or -1
end

local headline = { "Life", "EnergyShield", "Mana", "Ward", "TotalEHP", "TotalDPS", "CombinedDPS", "FullDPS",
	"FullDotDPS", "FireResist", "ColdResist", "LightningResist", "ChaosResist" }
local function pick(o)
	local r = { }
	for _, k in ipairs(headline) do
		r[k] = o[k]
	end
	return r
end

local result = {
	main = ct.mainOutput,
	mainMinion = ct.mainEnv.minion and ct.mainEnv.minion.output or nil,
	calcs = ct.calcsOutput,
	mainUsed = used(ct.mainEnv),
	calcsBreakdown = {
		SkillBuffs = rowCount(ct.calcsEnv.player.breakdown, "SkillBuffs"),
		SkillDebuffs = rowCount(ct.calcsEnv.player.breakdown, "SkillDebuffs"),
	},
	costs = costs,
}
local mc = ct.miscCalculator
local misc = { base = pick(mc[2]) }
local ok, err = pcall(function()
	misc.callDefault = pick(mc[1]({ }))
	misc.callNoFullDPS = pick(mc[1]({ }, false))
end)
if not ok then
	misc.error = tostring(err)
end
result.misc = misc
return __oracle_json(result, 40)
