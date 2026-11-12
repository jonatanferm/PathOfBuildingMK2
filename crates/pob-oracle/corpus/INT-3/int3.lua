-- INT-3 acceptance (crates/pob-oracle/tests/INT-3.rs): what the Calcs tab reads after a build
-- load, as JSON. Runs in the oracle process after the load script (oracle.lua's `__oracle_json`
-- encoder, state_dump.lua's `__state_dump`). Owned by INT-3.
--
-- * `tabulate`: every distinct mod query of Modules/CalcSections.lua (the entries with a
--   `modName` that CalcBreakdownControl:SetBreakdownData sends to AddModSection,
--   CalcBreakdownControl.lua:75-81), run as AddModSection runs it (CalcBreakdownControl.lua
--   299-318) on `calcsTab.calcsEnv`, once per actor (player, and the minion when there is one;
--   GetActor, 47-54): the Tabulate rows in PoB's order, then the per-source Combine totals the
--   rows' source tooltips show (348-381), as raw numbers (the UI formats them).
--   A query that raises in PoB (e.g. `actor.mainSkill[...]` on an actor without a main skill)
--   records the error.
-- * `calcsEnv` (a second result): `calcsTab.calcsEnv` as a pob-state/1 document
--   (state_dump.lua `encodeDoc`), whose `env.player.breakdown` / `env.minion.breakdown` are the
--   CALCS-mode breakdown tables the breakdown control shows.
local b = __mainObject__.main.modes["BUILD"]
local ct = b.calcsTab
local env = ct.calcsEnv

local sections = LoadModule("Modules/CalcSections")

-- Every table with a `modName` and no `breakdown`, depth first: array members in order, then
-- the other table-valued members by sorted key.
local queries, seen = { }, { }
local function sig(e)
	local names = type(e.modName) == "table" and table.concat(e.modName, ",") or tostring(e.modName)
	return table.concat({ names, tostring(e.modType), tostring(e.cfg), tostring(e.enemy and true or false),
		tostring(e.actor), tostring(e.modSource) }, "|")
end
local function walk(t, depth)
	if type(t) ~= "table" or depth > 12 then
		return
	end
	if t.modName and not t.breakdown then
		local s = sig(t)
		if not seen[s] then
			seen[s] = true
			queries[#queries + 1] = {
				sig = s,
				modName = type(t.modName) == "table" and t.modName or { t.modName },
				multi = type(t.modName) == "table",
				modType = t.modType,
				cfg = t.cfg,
				enemy = t.enemy and true or false,
				actor = t.actor,
				modSource = t.modSource,
			}
		end
	end
	for _, v in ipairs(t) do
		walk(v, depth + 1)
	end
	local keys = { }
	for k, v in pairs(t) do
		if type(k) == "string" and type(v) == "table" and k ~= "modName" then
			keys[#keys + 1] = k
		end
	end
	table.sort(keys)
	for _, k in ipairs(keys) do
		walk(t[k], depth + 1)
	end
end
walk(sections, 0)

-- CalcBreakdownControl.lua 303-318 (the query) and 348-381 (the source totals), per actor.
local function run(actor, q)
	local cfg = (q.cfg and actor.mainSkill[q.cfg.."Cfg"] and copyTable(actor.mainSkill[q.cfg.."Cfg"], true)) or { }
	cfg.source = q.modSource
	cfg.actor = q.actor
	local modStore = (q.enemy and actor.enemy.modDB) or (q.cfg and actor.mainSkill.skillModList) or actor.modDB
	local rowList = modStore:Tabulate(q.modType, cfg, unpack(q.modName))
	local rows = { }
	for i, row in ipairs(rowList) do
		rows[i] = { value = row.value, mod = row.mod }
	end
	local totals = { }
	if not q.modSource then
		local types, typeList, sources, sourceList = { }, { }, { }, { }
		for _, row in ipairs(rowList) do
			if not types[row.mod.type] then
				types[row.mod.type] = true
				typeList[#typeList + 1] = row.mod.type
			end
			local sourceType = row.mod.source:match("[^:]+")
			if not sources[sourceType] then
				sources[sourceType] = true
				sourceList[#sourceList + 1] = sourceType
			end
		end
		table.sort(sourceList)
		for _, sourceType in ipairs(sourceList) do
			cfg.source = sourceType
			for _, modType in ipairs(typeList) do
				for _, modName in ipairs(q.modName) do
					totals[#totals + 1] = { source = sourceType, modType = modType, modName = modName,
						total = modStore:Combine(modType, cfg, modName) }
				end
			end
		end
	end
	return { rows = rows, totals = totals }
end

local function runAll(actor)
	local out = { }
	for _, q in ipairs(queries) do
		local ok, r = pcall(run, actor, q)
		if ok then
			out[q.sig] = r
		else
			out[q.sig] = { error = tostring(r) }
		end
	end
	return out
end

local qlist = { }
for i, q in ipairs(queries) do
	qlist[i] = { sig = q.sig, modName = q.modName, multi = q.multi, modType = q.modType, cfg = q.cfg,
		enemy = q.enemy, actor = q.actor, modSource = q.modSource }
end
local result = {
	queries = qlist,
	player = runAll(env.player),
	minion = env.minion and runAll(env.minion) or nil,
}
local doc = __state_dump.encodeDoc({ build = "int3", hit = "int3:calcsEnv", probe = "int3:calcsEnv", n = 1,
	kind = "out", stack = { }, env = env, envId = "calcsEnv", locals = { } })
return __oracle_json(result, 40), doc
