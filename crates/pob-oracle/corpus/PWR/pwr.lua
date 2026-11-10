-- PWR acceptance (crates/pob-oracle/tests/PWR.rs): PoB's power report on a loaded build, as JSON.
-- Runs in the oracle process after the load script (oracle.lua's `__oracle_json` encoder).
-- Owned by PWR.
--
-- Inputs (globals): __pwr_stat (a data.powerStatList `stat`, or nil for Offence/Defence),
-- __pwr_depth (calcsTab.nodePowerMaxDepth, or nil).
--
-- Runs `calcsTab:PowerBuilder()` directly (outside a coroutine, so it never yields), with the
-- misc calculator wrapped to record every call: the node set (id, modKey pairs), useFullDPS and
-- the output keys the power stats read. Then dumps every spec node's `power` (and alloc, modKey,
-- pathDist), every clusterNodeMap node's `power`, `powerMax`, the base output and
-- mainEnv.grantedPassives.
local b = __mainObject__.main.modes["BUILD"]
local ct = b.calcsTab

local statKeys = { "LifeUnreserved", "Life", "Armour", "EnergyShieldRecoveryCap", "EnergyShield", "Evasion",
	"LifeRegenRecovery", "EnergyShieldRegenRecovery", "CombinedDPS", "FullDPS", "FullDotDPS" }
local selected = nil
if __pwr_stat then
	for _, entry in ipairs(data.powerStatList) do
		if entry.stat == __pwr_stat then
			selected = entry
			break
		end
	end
	assert(selected, "no powerStatList entry " .. tostring(__pwr_stat))
	statKeys[#statKeys + 1] = __pwr_stat
	local minionStat = __pwr_stat:match("^Minion(.+)")
	if minionStat then
		statKeys[#statKeys + 1] = minionStat
	end
end

local function slim(out)
	local function pick(t)
		local r = { }
		for _, k in ipairs(statKeys) do
			local v = t[k]
			local tv = type(v)
			if tv == "number" or tv == "boolean" or tv == "string" then
				r[k] = v
			end
		end
		return r
	end
	local r = { top = pick(out) }
	if type(out.Minion) == "table" then
		r.minion = pick(out.Minion)
	end
	return r
end

local function nodeKey(set)
	local list = { }
	for node in pairs(set) do
		list[#list + 1] = { node.id, node.modKey }
	end
	table.sort(list, function(a, c)
		if a[1] ~= c[1] then return a[1] < c[1] end
		return a[2] < c[2]
	end)
	return list
end

ct.powerStat = selected
ct.nodePowerMaxDepth = __pwr_depth
ct.powerMax = nil
local realFunc, base = ct.miscCalculator[1], ct.miscCalculator[2]
local calls = { }
ct.miscCalculator[1] = function(override, useFullDPS)
	local out = realFunc(override, useFullDPS)
	local remove = override.removeNodes ~= nil and override.addNodes == nil
	calls[#calls + 1] = {
		remove = remove,
		nodes = nodeKey(override.addNodes or override.removeNodes),
		useFullDPS = useFullDPS == nil and "nil" or tostring(useFullDPS),
		out = slim(out),
	}
	return out
end
local t0 = os.clock()
local ok, err = pcall(ct.PowerBuilder, ct)
local seconds = os.clock() - t0
ct.miscCalculator[1] = realFunc

local nodes = { }
for id, node in pairs(b.spec.nodes) do
	nodes[tostring(id)] = {
		power = node.power,
		alloc = node.alloc and true or false,
		modKey = node.modKey,
		pathDist = node.pathDist,
		type = node.type,
	}
end
local cluster = { }
for name, node in pairs(b.spec.tree.clusterNodeMap) do
	cluster[name] = { power = node.power or { }, id = node.id, modKey = node.modKey }
end
local granted = { }
for id in pairs(ct.mainEnv.grantedPassives) do
	granted[#granted + 1] = id
end
table.sort(granted)

return __oracle_json({
	ok = ok,
	err = err and tostring(err) or nil,
	seconds = seconds,
	calls = calls,
	base = slim(base),
	nodes = nodes,
	cluster = cluster,
	powerMax = ct.powerMax,
	granted = granted,
}, 8)
