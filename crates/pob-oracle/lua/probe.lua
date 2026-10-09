-- pob-oracle: probe runtime for the patched PoB sources (calc doc §9.2).
--
-- Loaded before PoB boots, after state_dump.lua (and provenance.lua when
-- enabled). The patcher (src/patcher.rs) inserts into the copied sources:
--   * line probes:      `if __PROBE then __PROBE("<id>", { a = a, ... }) end `
--                       prefixed to a line of docs/port/stages.toml;
--   * function probes:  `<target> = __PROBE_FN("<id>", { "<param>", ... }, function(...) ... end)`
--                       around a function definition, and
--                       `__PROBE_CALL("<id>", { ... }, f)(args)` at call sites.
-- Hits are numbered `<id>#<n>` per process, in the order they fire: a line
-- probe's id is its stages.toml id, a function probe fires `<id>:in#<n>` on
-- entry and `<id>:out#<n>` on return (the same n, so in/out pair up even
-- when the function errors and has no :out). Selected hits are serialised
-- as pob-state/1 documents to <outDir>/state/<hit>.json; every hit counts.
--
-- __probe_cfg = { outDir, build, root, select = { glob, ... }, maxDumps, provenance }

local S = __state_dump
local cfg = __probe_cfg or { }
local type, select, unpack, pcall, error, tostring = type, select, unpack, pcall, error, tostring
local concat, sort = table.concat, table.sort

local counts = { }
local stack = { }    -- { hit = "<id>#<n>", env = env|nil }
local dumps = { }    -- { hit, file, stack, env }
local skipped = 0
local maxDumps = cfg.maxDumps or 1000

-- Selection. A glob ("*" matches anything) is matched against the hit id
-- `<probe>#<n>`; a glob without "#" matches every hit of the probe ids it
-- matches. `<glob>@main` instead selects, for each probe id the glob
-- matches, the first hit whose env is the build's final main env
-- (`calcsTab.mainEnv`): such hits are serialised when they fire and
-- written at the end if their env turned out to be the main one. The
-- candidates are the envs of `calcs.buildOutput(build, "MAIN")`, i.e. of an
-- `initEnv(build, "MAIN")` call without override and specEnv (the MAIN envs
-- `calcs.buildActiveSkill` derives pass `env.override`); only the two most
-- recent candidates' dumps are kept in memory.
local patterns = { }
local function globPattern(g)
	return "^" .. g:gsub("[%^%$%(%)%%%.%[%]%+%-%?]", "%%%0"):gsub("%*", ".*") .. "$"
end
for _, g in ipairs(cfg.select or { }) do
	local base = g:match("^(.*)@main$")
	if base then
		patterns[#patterns + 1] = { pat = globPattern(base), main = true }
	else
		if not g:find("#", 1, true) then g = g .. "#*" end
		patterns[#patterns + 1] = { pat = globPattern(g) }
	end
end

local mainTaken = { }   -- "<envId>|<probe id>" -> true
local pending = { }     -- envId -> { { hit, file, json, stack, env } }
local pendingOrder = { } -- candidate env ids with pending dumps, oldest first
local mainCandidates = setmetatable({ }, { __mode = "k" }) -- env -> true

-- Whether `env` is (or, inside its initEnv, is becoming) a buildOutput
-- MAIN env: the innermost initEnv frame decides for envs not yet returned.
local function isMainCandidate(env)
	if mainCandidates[env] then return true end
	for i = #stack, 1, -1 do
		local f = stack[i]
		if f.initEnv then
			if f.initMain then
				mainCandidates[env] = true
				return true
			end
			return false
		end
	end
	return false
end

-- "now", "main" or nil for a hit.
local function selectHit(hit, probeId, env)
	local how
	for i = 1, #patterns do
		local p = patterns[i]
		if p.main then
			if not how and type(env) == "table" and probeId:find(p.pat) and isMainCandidate(env) then
				local key = S.envId(env) .. "|" .. probeId
				if not mainTaken[key] then
					mainTaken[key] = true
					how = "main"
				end
			end
		elseif hit:find(p.pat) then
			return "now"
		end
	end
	return how
end

local function pack(...)
	return { n = select("#", ...), ... }
end

local function currentEnv(locals)
	if locals and type(locals.env) == "table" then return locals.env end
	for i = #stack, 1, -1 do
		if stack[i].env then return stack[i].env end
	end
	return nil
end

local function stackIds()
	local ids = { }
	for i = 1, #stack do ids[i] = stack[i].hit end
	return ids
end

local function fileName(hit)
	return (hit:gsub("[^%w%._:#@%-]", "_")) .. ".json"
end

local function writeFile(path, s)
	local f, err = io.open(path, "wb")
	if not f then error("probe: cannot write " .. tostring(path) .. ": " .. tostring(err)) end
	f:write(s)
	f:close()
end

local function doDump(how, hit, id, n, kind, env, locals)
	if #dumps >= maxDumps then
		skipped = skipped + 1
		return
	end
	local ids = stackIds()
	local envId = S.envId(env)
	local json = S.encodeDoc({ build = cfg.build, hit = hit, probe = id, n = n, kind = kind,
		stack = ids, env = env, envId = envId, locals = locals })
	local file = "state/" .. fileName(hit)
	local entry = { hit = hit, file = file, stack = ids, env = envId }
	if how == "now" then
		writeFile(cfg.outDir .. "/" .. file, json)
		dumps[#dumps + 1] = entry
		return
	end
	entry.json = json
	if not pending[envId] then
		pending[envId] = { }
		pendingOrder[#pendingOrder + 1] = envId
		while #pendingOrder > 2 do
			pending[table.remove(pendingOrder, 1)] = nil
		end
	end
	local list = pending[envId]
	list[#list + 1] = entry
end

function __PROBE(id, locals)
	local n = (counts[id] or 0) + 1
	counts[id] = n
	local env = currentEnv(locals)
	if env then S.envId(env) end
	local hit = id .. "#" .. n
	local how = selectHit(hit, id, env)
	if how then doDump(how, hit, id, n, "line", env, locals) end
end

local function toLocals(params, args)
	local locals = { }
	local np = #params
	for i = 1, np do
		locals[params[i]] = args[i]
	end
	if args.n > np then
		local va = { n = args.n - np }
		for i = np + 1, args.n do va[i - np] = args[i] end
		locals.varargs = va
	end
	return locals
end

-- Extra state worth dumping for probes whose parameters do not reach it.
local function extraLocals(id, locals)
	if id == "ConfigTab:BuildModList" and type(locals.self) == "table" then
		locals.modList = locals.self.modList
		locals.enemyModList = locals.self.enemyModList
	end
end

function __PROBE_FN(id, params, f)
	local inId, outId = id .. ":in", id .. ":out"
	local isInitEnv = id == "initEnv"
	local envIndex
	for i, p in ipairs(params) do
		if p == "env" then envIndex = i end
	end
	local w = function(...)
		local n = (counts[inId] or 0) + 1
		counts[inId] = n
		local args = pack(...)
		local hitIn = inId .. "#" .. n
		local env = currentEnv(envIndex and { env = args[envIndex] } or nil)
		local how = selectHit(hitIn, inId, env)
		if how then
			local locals = toLocals(params, args)
			extraLocals(id, locals)
			doDump(how, hitIn, inId, n, "in", env, locals)
		end
		local depth = #stack + 1
		stack[depth] = { hit = hitIn, env = env, initEnv = isInitEnv,
			initMain = isInitEnv and args[2] == "MAIN" and args[3] == nil and args[4] == nil }
		local r = pack(pcall(f, ...))
		for i = #stack, depth, -1 do stack[i] = nil end
		if not r[1] then error(r[2], 0) end
		counts[outId] = (counts[outId] or 0) + 1
		if isInitEnv and type(r[2]) == "table" then
			-- every env gets its identity when initEnv returns it
			S.envId(r[2])
			env = r[2]
			if args[2] == "MAIN" and args[3] == nil and args[4] == nil then
				mainCandidates[env] = true
			end
		end
		local hitOut = outId .. "#" .. n
		local howOut = selectHit(hitOut, outId, env)
		if howOut then
			local locals = toLocals(params, args)
			local ret = { n = r.n - 1 }
			for i = 2, r.n do ret[i - 1] = r[i] end
			locals.ret = ret
			extraLocals(id, locals)
			doDump(howOut, hitOut, outId, n, "out", env, locals)
		end
		return unpack(r, 2, r.n)
	end
	S.wrapped[w] = f
	return w
end

-- A call-site probe: `__PROBE_CALL(id, params, f)(args...)`.
function __PROBE_CALL(id, params, f)
	if type(f) ~= "function" then return f end
	return __PROBE_FN(id, params, f)
end

-- Identity for `__PROV` when provenance is off (provenance.lua replaces it).
if not __PROV then
	function __PROV(_, t) return t end
end

-- After the build has loaded: the hit index and the provenance of the
-- main env's outputs.
function __probe_finish(mainEnv)
	-- the buffered @main dumps of the env that became the main env
	local mainId = S.envIds[mainEnv]
	local mainDumps = 0
	for _, e in ipairs((mainId and pending[mainId]) or { }) do
		writeFile(cfg.outDir .. "/" .. e.file, e.json)
		e.json = nil
		dumps[#dumps + 1] = e
		mainDumps = mainDumps + 1
	end
	pending = { }
	local ids = { }
	for id in pairs(counts) do ids[#ids + 1] = id end
	sort(ids)
	local countList = { }
	for _, id in ipairs(ids) do countList[id] = counts[id] end
	local doc = {
		format = "pob-hits/1",
		build = cfg.build,
		counts = countList,
		dumps = dumps,
		skipped = skipped,
		envs = S.nextEnv,
		mainEnv = mainId or false,
		mainDumps = mainDumps,
		select = cfg.select or { },
	}
	writeFile(cfg.outDir .. "/hits.json", S.json(doc))
	if cfg.provenance and S.provenanceOf then
		local prov = S.provenanceOf(mainEnv)
		prov.format = "pob-provenance/1"
		prov.build = cfg.build
		prov.env = S.envIds[mainEnv] or false
		writeFile(cfg.outDir .. "/provenance.json", S.json(prov))
	end
	return #dumps
end
