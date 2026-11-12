-- pob-oracle: the pob-state/2 encoder (calc-architecture.md §9.2).
--
-- Loaded into the oracle's LuaJIT state *before* PoB boots (as part of the
-- `pre` chunk), so it captures the raw `next` before provenance.lua replaces
-- the iteration globals. It never calls PoB code except `cacheSkillUUID`
-- (pure string building) and never triggers metamethods: every table is
-- read with rawget / raw next, proxies are read through their backing store.
--
-- Encoding (crates/pob-oracle/README.md has the full table):
--   * values follow crates/pob-dump/FORMAT.md: integral doubles as JSON
--     integers, other finite numbers as the shortest of %.15g/%.16g/%.17g
--     that round-trips, non-finite and -0 as {"$float": ...}, non-UTF-8
--     strings as {"$bytes": hex}; a table is a JSON array exactly when it is
--     a pure sequence 1..n (n >= 1) without metatable or identity, otherwise
--     an object of its string keys (bytewise sorted, "$x" escaped to "$$x")
--     plus "$seq", "$num", "$keys" and "$meta" members;
--   * identity: every table is written in full once, at its canonical path
--     (the first path a breadth-first walk reaches it by, members in the
--     order string keys, $seq, $num, $keys, $meta, so the shortest path
--     wins), with "$id": n when it is reached more than once, and as
--     {"$ref": n} everywhere else; aliasing and cycles survive and paths are
--     stable between runs. Paths: `env.player.modDB`, `x[3]` for numeric
--     keys, `x[@i]` / `x[@i:key]` for the i-th $keys entry, `x<meta>`;
--   * markers (never descended): {"$data": "<dump path>"} for tables owned by
--     PoB's `data`, {"$node": id} / {"$treeNode": id} for passive tree nodes,
--     {"$object": className} for PoB class instances other than mod stores
--     and items, {"$foreign": {"env", "path"}} for the envs / breakdowns / active
--     skills of other calc envs than the dump's own (their actors, mod stores and
--     actors' outputs are written in full where reached: inlineForeign);
--   * mod stores: {"$store": "ModDB"|"ModList", "mods": ..., <fields>} with
--     ModDB mods grouped by name (names sorted, PoB's order within a name),
--     ModList mods in PoB's order, each mod in pob_calc::Mod's JSON shape plus
--     "$at": "<file>:<line>" of the code that inserted it (provenance runs);
--   * items: {"$item": name, "$modLines": <mod lines>, <whitelisted fields>}, where
--     "$modLines" holds what Item:FindModifierSubstring / CheckModLineVariant read
--     (Classes/Item.lua:289-333, 2120-2146): the explicit / enchant / scourge /
--     implicit / crucible mod lines (each line's line, disabled, variantList,
--     versionList, variantGroupList) and the item's variant selection fields;
--     a value object (no identity), not a table member (pob-state/2, FUP3);
--   * functions: {"$fn": "<file>:<first>-<last>"} (probe wrappers report the
--     function they wrap); a metatable's functions also carry their upvalue
--     names ("upnames"), which identifies skillCond's function __index
--     layers; a closure of `S.upvalueSources` (ModParser.lua's jewel funcs)
--     carries its upvalues as "$up": [[name, value], ...] in upvalue order,
--     each value a value object whose functions carry no "$up" (pob-state/2,
--     FUP3), when the host gave the encoder `S.getupvalue`.

local type, tostring, tonumber, select, ipairs, pcall, error = type, tostring, tonumber, select, ipairs, pcall, error
local rawget, getmetatable, setmetatable = rawget, getmetatable, setmetatable
local fmt, concat, sort, byte = string.format, table.concat, table.sort, string.byte
local floor, huge = math.floor, math.huge
local rawnext = next

-- The oracle's LuaJIT state has no `debug` library (mlua's safe mode), so
-- function sources come from `jit.util.funcinfo` and stack positions from
-- the position prefix `error(msg, level)` adds. Upvalue values come from
-- `S.getupvalue(f, n)` (`lua_getupvalue`: name, value), which the probe host
-- installs after boot (`JitPob::install_probe_upvalue_reader`, called by
-- `runner::probe_in_process`); without it no "$up" is written. It is not a
-- global, so PoB's environment is unchanged (ItemsTab.lua:4175 tests `debug`).
local jutil
do
	local ok, m = pcall(require, "jit.util")
	if ok and type(m) == "table" then jutil = m end
end

local S = { }
__state_dump = S

-- proxy -> backing table (provenance.lua)
S.proxyStore = setmetatable({ }, { __mode = "k" })
-- store -> { mod -> "file:line" } (provenance.lua)
S.modAt = setmetatable({ }, { __mode = "k" })
-- probe wrapper -> wrapped function (probe.lua)
S.wrapped = setmetatable({ }, { __mode = "k" })
-- env -> "env<k>" (probe.lua registers every env initEnv returns)
S.envIds = setmetatable({ }, { __mode = "k" })
S.nextEnv = 0
-- absolute path prefix of PoB's src/ (with trailing slash), stripped from sources
S.root = (__probe_cfg and __probe_cfg.root) or ""

function S.envId(env)
	if type(env) ~= "table" then return nil end
	local id = S.envIds[env]
	if not id then
		S.nextEnv = S.nextEnv + 1
		id = "env" .. S.nextEnv
		S.envIds[env] = id
	end
	return id
end

-- "@/abs/src/Modules/X.lua" (a chunk name) or ".../src/Modules/X.lua" (a
-- truncated short_src) -> "Modules/X.lua"
function S.relSource(source)
	if type(source) ~= "string" then return "?" end
	if byte(source, 1) == 64 then source = source:sub(2) end -- '@'
	local root = S.root
	if root ~= "" and source:sub(1, #root) == root then
		return source:sub(#root + 1)
	end
	local last
	local i = 1
	while true do
		local s, e = source:find("/src/", i, true)
		if not s then break end
		last = e
		i = s + 1
	end
	if last then return source:sub(last + 1) end
	return source
end

-- "file:first-last" of a Lua function, "[C]" otherwise.
function S.fnSource(f)
	local info = jutil and jutil.funcinfo(f)
	if type(info) ~= "table" or not info.source then return "[C]" end
	return S.relSource(info.source) .. ":" .. tostring(info.linedefined) .. "-" .. tostring(info.lastlinedefined)
end

-- Closures whose upvalue values are dumped ("$up"): source prefixes. ModParser.lua's
-- closures are the jewel funcs, whose captures the port holds as `JewelFuncSpec.args`
-- (pob_oracle::mod_json decodes them; FOLLOWUPS "CS2" 2).
S.upvalueSources = { "Modules/ModParser.lua:" }

function S.dumpsUpvalues(src)
	if type(S.getupvalue) ~= "function" then return false end
	for _, prefix in ipairs(S.upvalueSources) do
		if src:sub(1, #prefix) == prefix then return true end
	end
	return false
end

-- Upvalue names of a Lua function.
function S.upvalueNames(f)
	local names = { }
	if not jutil then return names end
	local i = 0
	while true do
		local name = jutil.funcuvname(f, i)
		if name == nil then break end
		names[#names + 1] = name
		i = i + 1
	end
	return names
end

-- "file:line" of stack level `k` counted from the caller of S.where
-- (k = 1: the function that called S.where), "" for C frames / beyond the
-- stack. error(msg, level) prefixes "short_src:line: ": inside pcall,
-- level 1 is pcall itself and level 2 is S.where.
function S.where(k)
	local _, msg = pcall(error, "", k + 2)
	if type(msg) ~= "string" or msg == "" then return "" end
	local src, line = msg:match("^(.*):(%d+): $")
	if not src then return "" end
	return S.relSource(src) .. ":" .. line
end

---------------------------------------------------------------------------
-- Scalars
---------------------------------------------------------------------------

local function isUtf8(s)
	local i, n = 1, #s
	while i <= n do
		local c = byte(s, i)
		if c < 0x80 then
			i = i + 1
		else
			local len, lo, hi
			if c >= 0xC2 and c <= 0xDF then len, lo, hi = 2, 0x80, 0xBF
			elseif c == 0xE0 then len, lo, hi = 3, 0xA0, 0xBF
			elseif c == 0xED then len, lo, hi = 3, 0x80, 0x9F
			elseif c >= 0xE1 and c <= 0xEF then len, lo, hi = 3, 0x80, 0xBF
			elseif c == 0xF0 then len, lo, hi = 4, 0x90, 0xBF
			elseif c >= 0xF1 and c <= 0xF3 then len, lo, hi = 4, 0x80, 0xBF
			elseif c == 0xF4 then len, lo, hi = 4, 0x80, 0x8F
			else return false end
			local c2 = byte(s, i + 1)
			if not c2 or c2 < lo or c2 > hi then return false end
			for j = i + 2, i + len - 1 do
				local cj = byte(s, j)
				if not cj or cj < 0x80 or cj > 0xBF then return false end
			end
			i = i + len
		end
	end
	return true
end

local escapes = { ['"'] = '\\"', ['\\'] = '\\\\', ['\n'] = '\\n', ['\t'] = '\\t', ['\r'] = '\\r', ['\b'] = '\\b', ['\f'] = '\\f' }
local function escapeChar(c)
	return escapes[c] or fmt("\\u%04x", byte(c))
end

-- A JSON string for a Lua string known to be UTF-8 (paths, ids).
local function quote(s)
	return '"' .. s:gsub('[%c"\\]', escapeChar) .. '"'
end

local function jstr(s)
	if s:find("[\128-\255]") and not isUtf8(s) then
		return '{"$bytes":"' .. s:gsub(".", function(c) return fmt("%02x", byte(c)) end) .. '"}'
	end
	return quote(s)
end
S.jstr = jstr
S.quote = quote

local function jnum(v)
	if v ~= v then return '{"$float":"nan"}' end
	if v == huge then return '{"$float":"inf"}' end
	if v == -huge then return '{"$float":"-inf"}' end
	if v == 0 then
		if 1 / v < 0 then return '{"$float":"-0"}' end
		return "0"
	end
	if v == floor(v) and v > -9007199254740992 and v < 9007199254740992 then
		return fmt("%.0f", v)
	end
	local s = fmt("%.15g", v)
	if tonumber(s) == v then return s end
	s = fmt("%.16g", v)
	if tonumber(s) == v then return s end
	return fmt("%.17g", v)
end
S.jnum = jnum

-- Canonical text of a numeric key (FORMAT.md: inf/-inf spelled out).
local function keyText(k)
	if k ~= k then return "nan" end
	if k == huge then return "inf" end
	if k == -huge then return "-inf" end
	if k == 0 then return "0" end
	if k == floor(k) and k > -9007199254740992 and k < 9007199254740992 then
		return fmt("%.0f", k)
	end
	local s = fmt("%.15g", k)
	if tonumber(s) == k then return s end
	s = fmt("%.16g", k)
	if tonumber(s) == k then return s end
	return fmt("%.17g", k)
end
S.keyText = keyText

-- Lua-like path step, as crates/pob-dump/FORMAT.md writes canonical paths.
local function pathStep(base, k)
	local tk = type(k)
	if tk == "string" then
		if k:match("^[%a_][%w_]*$") then return base .. "." .. k end
		return base .. "[" .. jstr(k) .. "]"
	elseif tk == "number" then
		return base .. "[" .. keyText(k) .. "]"
	elseif tk == "boolean" then
		return base .. "[" .. tostring(k) .. "]"
	end
	return nil
end
S.pathStep = pathStep

---------------------------------------------------------------------------
-- Classification helpers
---------------------------------------------------------------------------

local modTypes = { BASE = true, INC = true, MORE = true, FLAG = true, OVERRIDE = true, LIST = true,
	MAX = true, MIN = true, Flag = true, CHANCE = true, DUMMY = true }

local function isMod(t)
	return type(rawget(t, "name")) == "string" and modTypes[rawget(t, "type")] ~= nil
		and S.proxyStore[t] == nil
end
S.isMod = isMod

-- The PoB class name of an instance (its metatable is the class table).
local function classOf(t)
	local mt = getmetatable(t)
	if type(mt) ~= "table" then return nil end
	local cls = rawget(mt, "_className")
	if type(cls) == "string" then return cls end
	return nil
end
S.classOf = classOf

-- The class table itself (common.classes[X]) has _className as a raw field.
local function classTableName(t)
	local cls = rawget(t, "_className")
	if type(cls) == "string" and rawget(t, "__index") == t then return cls end
	return nil
end

local function sortedKeys(t)
	local skeys, nkeys, okeys = { }, { }, { }
	local k = rawnext(t)
	while k ~= nil do
		local tk = type(k)
		if tk == "string" then
			if k:find("[\128-\255]") and not isUtf8(k) then okeys[#okeys + 1] = k else skeys[#skeys + 1] = k end
		elseif tk == "number" then nkeys[#nkeys + 1] = k
		else okeys[#okeys + 1] = k end
		k = rawnext(t, k)
	end
	sort(skeys)
	sort(nkeys)
	return skeys, nkeys, okeys
end
S.sortedKeys = sortedKeys

---------------------------------------------------------------------------
-- The `data` index: table -> canonical path (breadth-first from `data`,
-- string keys bytewise then numeric keys ascending, the shortest path wins).
---------------------------------------------------------------------------

local dataIndex
S.dataIndexDepth = 4

function S.dataIndex()
	if dataIndex then return dataIndex end
	dataIndex = { }
	local root = rawget(_G, "data")
	if type(root) ~= "table" then return dataIndex end
	dataIndex[root] = "data"
	local queue, depthOf = { root }, { 0 }
	local head = 1
	while queue[head] do
		local t, d = queue[head], depthOf[head]
		head = head + 1
		if d < S.dataIndexDepth then
			local base = dataIndex[t]
			local skeys, nkeys = sortedKeys(t)
			for pass = 1, 2 do
				local keys = pass == 1 and skeys or nkeys
				for _, k in ipairs(keys) do
					local v = rawget(t, k)
					if type(v) == "table" and dataIndex[v] == nil and not classOf(v) and not isMod(v) then
						dataIndex[v] = pathStep(base, k)
						queue[#queue + 1] = v
						depthOf[#depthOf + 1] = d + 1
					end
				end
			end
		end
	end
	return dataIndex
end

---------------------------------------------------------------------------
-- Per-dump context
---------------------------------------------------------------------------

local Ctx = { }
Ctx.__index = Ctx

local function newCtx(rootEnv)
	local ctx = setmetatable({
		count = { },
		ids = { },
		nextId = 1,
		buf = { },
		rootEnv = rootEnv,
		markers = { },   -- table -> marker JSON (nodes, foreign objects, build)
		seen = { },      -- cycle guard for value-object encoding (mods)
	}, Ctx)
	return ctx
end

function Ctx:put(s)
	local buf = self.buf
	buf[#buf + 1] = s
end

-- Markers for passive tree nodes, the build, and other envs' objects.
local function addForeign(ctx, own, obj, envId, path)
	if type(obj) == "table" and not own[obj] and ctx.markers[obj] == nil then
		if path == "" then
			ctx.markers[obj] = '{"$env":' .. quote(envId) .. '}'
		else
			ctx.markers[obj] = '{"$foreign":{"env":' .. quote(envId) .. ',"path":' .. quote(path) .. '}}'
		end
	end
end

-- Objects of other envs that are dumped in full where the dump reaches them (not as
-- {"$foreign": ...}): their mod stores (a copied skill's `skillModList` chains to the source
-- env's `modDB` through `baseSkillModList.parent`, CalcActiveSkill.lua:184/492, and every query
-- of the copy falls through to it) and their actors' `output` tables (`activeSkill.mirage.output`,
-- CalcMirages.lua; read by CalcOffence.lua:6213-6247) and the actors themselves. Their envs,
-- active skills and breakdowns stay markers. ORC-X2 (CCR-0041 §5, CCR-0044, CCR-0049 §4, CCR-0064 §1, CCR-0075).
local function inlineForeign(obj, path)
	local cls = type(obj) == "table" and classOf(obj)
	if cls == "ModDB" or cls == "ModList" then return true end
	-- the actors too: a copied skill keeps its source's `actor` (CalcActiveSkill.lua:181), and
	-- that is the context actor of every query of its stores (ModStore:EvalMod reads
	-- `self.actor`: conditions, multipliers, stats)
	if path == "player" or path == "enemy" or path == "minion" then return true end
	return path:match("%.output$") ~= nil
end

local function envObjects(env, f)
	if type(env) ~= "table" then return end
	f(env, "")
	for _, k in ipairs({ "modDB", "enemyDB", "itemModDB", "minionDB" }) do
		f(rawget(env, k), k)
	end
	for _, role in ipairs({ "player", "enemy", "minion" }) do
		local actor = rawget(env, role)
		if type(actor) == "table" then
			f(actor, role)
			f(rawget(actor, "modDB"), role .. ".modDB")
			f(rawget(actor, "output"), role .. ".output")
			f(rawget(actor, "breakdown"), role .. ".breakdown")
			local list = rawget(actor, "activeSkillList")
			if type(list) == "table" then
				for i = 1, #list do
					local skill = rawget(list, i)
					f(skill, role .. ".activeSkillList[" .. i .. "]")
					if type(skill) == "table" then
						f(rawget(skill, "skillModList"), role .. ".activeSkillList[" .. i .. "].skillModList")
					end
				end
			end
		end
	end
end

local function prepare(ctx)
	S.dataIndex()
	local markers = ctx.markers
	-- the build and the app objects
	local mainObject = rawget(_G, "__mainObject__")
	if type(mainObject) == "table" then markers[mainObject] = '{"$object":"Main"}' end
	-- the build being calculated (the `build` global is only refreshed
	-- after a load finishes)
	local b
	local main = type(mainObject) == "table" and mainObject.main
	if type(main) == "table" and type(main.modes) == "table" then b = main.modes.BUILD end
	if type(b) ~= "table" then b = rawget(_G, "build") end
	if ctx.rootEnv and type(rawget(ctx.rootEnv, "build")) == "table" then
		markers[rawget(ctx.rootEnv, "build")] = '{"$object":"Build"}'
		b = rawget(ctx.rootEnv, "build")
	end
	if type(b) == "table" then markers[b] = '{"$object":"Build"}' end
	markers[_G] = '{"$object":"_G"}'
	-- passive tree nodes: the build's spec and the env's spec (when different)
	local specs = { }
	if type(b) == "table" and type(rawget(b, "spec")) == "table" then specs[#specs + 1] = rawget(b, "spec") end
	if ctx.rootEnv and type(rawget(ctx.rootEnv, "spec")) == "table" then specs[#specs + 1] = rawget(ctx.rootEnv, "spec") end
	for _, spec in ipairs(specs) do
		local tree = rawget(spec, "tree")
		if type(tree) == "table" and type(rawget(tree, "nodes")) == "table" then
			for id, node in rawnext, rawget(tree, "nodes") do
				if type(node) == "table" and markers[node] == nil then
					markers[node] = '{"$treeNode":' .. jnum(tonumber(id) or 0) .. '}'
				end
			end
		end
		local nodes = rawget(spec, "nodes")
		if type(nodes) == "table" then
			for id, node in rawnext, nodes do
				if type(node) == "table" then
					markers[node] = '{"$node":' .. jnum(tonumber(id) or 0) .. '}'
				end
			end
		end
	end
	-- other envs: everything at the same position in the dump's own env is
	-- kept inline (accelerated initEnv shares cached mod DBs between envs)
	local own = { }
	if ctx.rootEnv then
		envObjects(ctx.rootEnv, function(obj) if type(obj) == "table" then own[obj] = true end end)
	end
	local envs = { }
	for env, id in rawnext, S.envIds do
		if env ~= ctx.rootEnv then envs[#envs + 1] = { env, id } end
	end
	sort(envs, function(a, b) return tonumber(a[2]:sub(4)) < tonumber(b[2]:sub(4)) end)
	for _, e in ipairs(envs) do
		envObjects(e[1], function(obj, path)
			if not inlineForeign(obj, path) then addForeign(ctx, own, obj, e[2], path) end
		end)
	end
end

---------------------------------------------------------------------------
-- Encoder
--
-- Two passes over the graph below the document root:
--   1. breadth-first, children in member order (string keys bytewise, then
--      $seq, $num, $keys, $meta): every identity table (plain table, mod
--      store, item, proxy) gets its canonical path, the first path at which
--      the walk reaches it (so the shortest path wins), and a reference count;
--   2. depth-first emission: a table is written in full only at its
--      canonical path ("$id": n when it is referenced more than once) and as
--      {"$ref": n} everywhere else, before or after. Ids are numbered in
--      emission order.
-- Mods are value objects (written in full wherever they occur, never
-- referenced); markers are leaves.
---------------------------------------------------------------------------

local encodePlain

-- Escape a string key: "$x" -> "$$x".
local function keyJson(k)
	if byte(k, 1) == 36 then k = "$" .. k end
	return jstr(k)
end

-- A child path; identifiers at the root have no leading dot.
local function childPath(base, k)
	if base == "" and type(k) == "string" and k:match("^[%a_][%w_]*$") then return k end
	return pathStep(base, k)
end

-- A content label for a table used as a key: its scalar members, the
-- markers / data paths of its table members, and a mod's tags, so the
-- order of `$keys` entries does not depend on table addresses (it can still
-- tie for keys that are equal in content).
local function shallowLabel(ctx, t, depth)
	local parts = { }
	local skeys, nkeys = sortedKeys(t)
	local function one(v)
		local tv = type(v)
		if tv == "string" then return jstr(v) end
		if tv == "number" then return jnum(v) end
		if tv == "boolean" then return tostring(v) end
		if tv == "table" then
			local m = ctx.markers[v]
			if m then return m end
			local d = S.dataIndex()[v]
			if d then return "@" .. d end
			local cls = classOf(v)
			if cls then return "<" .. cls .. ">" end
			if depth > 0 then return "{" .. shallowLabel(ctx, v, depth - 1) .. "}" end
			return "{}"
		end
		return tv
	end
	for _, k in ipairs(skeys) do parts[#parts + 1] = k .. "=" .. one(rawget(t, k)) end
	for _, k in ipairs(nkeys) do parts[#parts + 1] = "[" .. keyText(k) .. "]=" .. one(rawget(t, k)) end
	return concat(parts, ",")
end

-- A label for sorting non-string, non-number keys ($keys members).
local function keyLabel(ctx, k)
	local tk = type(k)
	if tk == "boolean" then return "0:" .. tostring(k) end
	if tk == "string" then return "6:" .. k end
	if tk == "table" then
		local m = ctx.markers[k]
		if m then return "1:" .. m end
		local d = S.dataIndex()[k]
		if d then return "2:" .. d end
		if classOf(k) == "Item" then
			return "3:" .. tostring(rawget(k, "id")) .. ":" .. tostring(rawget(k, "name"))
		end
		if isMod(k) then return "4:" .. shallowLabel(ctx, k, 2) end
		if classOf(k) then return "7:" .. classOf(k) .. ":" .. shallowLabel(ctx, k, 0) end
		return "9:" .. shallowLabel(ctx, k, 1)
	end
	if tk == "function" then
		return "5:" .. S.fnSource(S.wrapped[k] or k)
	end
	return "8:" .. tk
end

local function encodeFn(ctx, f, withUp)
	local inner = S.wrapped[f]
	if inner then f = inner end
	local src = S.fnSource(f)
	ctx:put('{"$fn":' .. jstr(src))
	-- "$up" before "upnames": members in bytewise order
	if not ctx.inUp and S.dumpsUpvalues(src) then
		-- the upvalues in order (lua_getupvalue numbers them from 1); their functions are
		-- written without "$up" (one level)
		ctx:put(',"$up":[')
		ctx.inUp = true
		local i = 1
		while true do
			local name, value = S.getupvalue(f, i)
			if name == nil then break end
			if i > 1 then ctx:put(",") end
			ctx:put("[" .. jstr(name) .. ",")
			encodePlain(ctx, value)
			ctx:put("]")
			i = i + 1
		end
		ctx.inUp = false
		ctx:put("]")
	end
	if withUp then
		local names = S.upvalueNames(f)
		local parts = { }
		for i, name in ipairs(names) do parts[i] = jstr(name) end
		ctx:put(',"upnames":[' .. concat(parts, ",") .. "]")
	end
	ctx:put("}")
end

local function encodeScalar(ctx, v)
	local tv = type(v)
	if tv == "nil" then ctx:put("null")
	elseif tv == "boolean" then ctx:put(v and "true" or "false")
	elseif tv == "number" then ctx:put(jnum(v))
	elseif tv == "string" then ctx:put(jstr(v))
	else ctx:put('{"$' .. tv .. '":true}') end
end

---------------------------------------------------------------------------
-- Value objects: mods (no identity, cycle-guarded)
---------------------------------------------------------------------------

encodePlain = function(ctx, v)
	local tv = type(v)
	if tv ~= "table" then
		if tv == "function" then return encodeFn(ctx, v, false) end
		return encodeScalar(ctx, v)
	end
	if v == S.EMPTY_ARRAY then ctx:put("[]") return end
	local m = ctx.markers[v]
	if not m and not ctx.plainOnly and not isMod(v) and S.dataIndex()[v] then
		m = '{"$data":' .. jstr(S.dataIndex()[v]) .. '}'
	end
	if m then ctx:put(m) return end
	local cls = classOf(v)
	if cls then ctx:put('{"$object":' .. quote(cls) .. '}') return end
	if ctx.seen[v] then ctx:put('{"$cycle":true}') return end
	ctx.seen[v] = true
	local skeys, nkeys, okeys = sortedKeys(v)
	local n = 0
	while rawget(v, n + 1) ~= nil do n = n + 1 end
	if n > 0 and #skeys == 0 and #okeys == 0 and #nkeys == n and getmetatable(v) == nil then
		ctx:put("[")
		for i = 1, n do
			if i > 1 then ctx:put(",") end
			encodePlain(ctx, rawget(v, i))
		end
		ctx:put("]")
	else
		ctx:put("{")
		local first = true
		local function sep() if first then first = false else ctx:put(",") end end
		for _, k in ipairs(skeys) do
			sep()
			ctx:put(keyJson(k) .. ":")
			encodePlain(ctx, rawget(v, k))
		end
		if n > 0 then
			sep()
			ctx:put('"$seq":[')
			for i = 1, n do
				if i > 1 then ctx:put(",") end
				encodePlain(ctx, rawget(v, i))
			end
			ctx:put("]")
		end
		local wrote = false
		for _, k in ipairs(nkeys) do
			if not (k >= 1 and k <= n and k == floor(k)) then
				if not wrote then sep() ctx:put('"$num":{') wrote = true else ctx:put(",") end
				ctx:put(quote(keyText(k)) .. ":")
				encodePlain(ctx, rawget(v, k))
			end
		end
		if wrote then ctx:put("}") end
		if #okeys > 0 then
			sep()
			ctx:put('"$keys":[')
			local labelled = { }
			for i, k in ipairs(okeys) do labelled[i] = { keyLabel(ctx, k), k } end
			sort(labelled, function(a, b) return a[1] < b[1] end)
			for i, e in ipairs(labelled) do
				if i > 1 then ctx:put(",") end
				ctx:put("[")
				encodePlain(ctx, e[2])
				ctx:put(",")
				encodePlain(ctx, rawget(v, e[2]))
				ctx:put("]")
			end
			ctx:put("]")
		end
		ctx:put("}")
	end
	ctx.seen[v] = nil
end

-- pob_calc::Mod's JSON shape: name, type, value, flags, keywordFlags,
-- source, tags (the array part), every other field sorted, plus "$at".
local modCore = { name = true, type = true, value = true, flags = true, keywordFlags = true, source = true }
local function encodeMod(ctx, m, at)
	if ctx.seen[m] then ctx:put('{"$cycle":true}') return end
	ctx.seen[m] = true
	ctx:put('{"name":' .. jstr(rawget(m, "name")) .. ',"type":' .. jstr(rawget(m, "type")) .. ',"value":')
	encodePlain(ctx, rawget(m, "value"))
	ctx:put(',"flags":' .. jnum(rawget(m, "flags") or 0) .. ',"keywordFlags":' .. jnum(rawget(m, "keywordFlags") or 0))
	local source = rawget(m, "source")
	if source ~= nil then
		ctx:put(',"source":')
		encodePlain(ctx, source)
	end
	ctx:put(',"tags":[')
	local i = 1
	while true do
		local tag = rawget(m, i)
		if tag == nil then break end
		if i > 1 then ctx:put(",") end
		encodePlain(ctx, tag)
		i = i + 1
	end
	ctx:put("]")
	local skeys = sortedKeys(m)
	for _, k in ipairs(skeys) do
		if not modCore[k] then
			ctx:put("," .. keyJson(k) .. ":")
			encodePlain(ctx, rawget(m, k))
		end
	end
	if at then ctx:put(',"$at":' .. jstr(at)) end
	ctx:put("}")
	ctx.seen[m] = nil
end

---------------------------------------------------------------------------
-- The graph: classification and members
---------------------------------------------------------------------------

-- Class plumbing that is not object state.
local function classSkip(obj)
	return function(k, v)
		if k == "Object" or k == "_parentInit" then return true end
		if type(v) == "table" and rawget(v, "_object") == obj then return true end
		return false
	end
end

local itemFields = { "id", "name", "title", "baseName", "type", "rarity", "quality", "corrupted",
	"base", "variant", "jewelRadiusIndex", "limit", "requirements",
	"modList", "slotModList", "weaponData", "armourData", "flaskData", "tinctureData", "jewelData" }
sort(itemFields)

-- "$modLines" of an item: the mod line lists Item:FindModifierSubstring merges (Item.lua:299-305),
-- the line fields it and CheckModLineVariant read (308, 2122-2145), and the item fields
-- CheckModLineVariant reads besides `variant` (an item field above).
local modLineLists = { "crucibleModLines", "enchantModLines", "explicitModLines", "implicitModLines", "scourgeModLines" }
local modLineFields = { "disabled", "line", "variantGroupList", "variantList", "versionList" }
local itemVariantFields = { "hasAltVariant", "hasAltVariant2", "hasAltVariant3", "hasAltVariant4", "hasAltVariant5",
	"selectedVersion", "usesVariantGroups", "variantAlt", "variantAlt2", "variantAlt3", "variantAlt4", "variantAlt5",
	"variantGroupSelections" }

-- A plain table for encodePlain: { <list> = { <line>... } (absent when the item has no such
-- field), variant = { <field> = value } }.
local function itemModLines(item)
	local out = { }
	for _, k in ipairs(modLineLists) do
		local list = rawget(item, k)
		if type(list) == "table" then
			local lines = { }
			local i = 1
			while true do
				local l = rawget(list, i)
				if l == nil then break end
				if type(l) == "table" then
					local line = { }
					for _, f in ipairs(modLineFields) do line[f] = rawget(l, f) end
					lines[#lines + 1] = line
				end
				i = i + 1
			end
			out[k] = S.array(lines)
		end
	end
	local variant = { }
	for _, f in ipairs(itemVariantFields) do variant[f] = rawget(item, f) end
	out.variant = variant
	return out
end

-- kind, content, extra:
--   "marker", json | "mod", mod | "store", content, class | "item", content | "table", content
local function classify(ctx, t)
	local m = ctx.markers[t]
	if m then return "marker", m end
	local store = S.proxyStore[t]
	local c = store or t
	if isMod(c) then return "mod", c end
	local d = S.dataIndex()[t]
	if d then return "marker", '{"$data":' .. jstr(d) .. '}' end
	if not store then
		local cls = classOf(t)
		if cls == "ModDB" or cls == "ModList" then return "store", c, cls end
		if cls == "Item" then return "item", c end
		if cls then return "marker", '{"$object":' .. quote(cls) .. '}' end
		local ctn = classTableName(t)
		if ctn then return "marker", '{"$class":' .. quote(ctn) .. '}' end
	end
	return "table", c
end

local function hasIdentity(kind)
	return kind == "table" or kind == "store" or kind == "item"
end

-- Members of an identity table in emission order, each
-- { group, jsonKey, path, value, withUp } with group one of "str", "seq",
-- "num", "kkey", "kval", "meta".
local function members(ctx, t, kind, c, cls, path, isMeta)
	local list = { }
	if kind == "item" then
		for _, k in ipairs(itemFields) do
			local v = rawget(c, k)
			if v ~= nil then list[#list + 1] = { "str", quote(k), childPath(path, k), v } end
		end
		return list
	end
	local skip
	if kind == "store" then
		local isDB = cls == "ModDB"
		local base = classSkip(t)
		skip = function(k, v)
			if base(k, v) then return true end
			if isDB and k == "mods" then return true end
			return false
		end
	end
	local skeys, nkeys, okeys = sortedKeys(c)
	for _, k in ipairs(skeys) do
		local v = rawget(c, k)
		if not (skip and skip(k, v)) then
			list[#list + 1] = { "str", keyJson(k), childPath(path, k), v, isMeta }
		end
	end
	if kind == "store" and cls == "ModList" then
		-- the array part is the mod list ("mods")
		nkeys = { }
	end
	local n = 0
	if not (kind == "store" and cls == "ModList") then
		while rawget(c, n + 1) ~= nil do n = n + 1 end
	end
	for i = 1, n do
		list[#list + 1] = { "seq", nil, path .. "[" .. i .. "]", rawget(c, i) }
	end
	for _, k in ipairs(nkeys) do
		if not (k >= 1 and k <= n and k == floor(k)) then
			list[#list + 1] = { "num", quote(keyText(k)), pathStep(path, k), rawget(c, k) }
		end
	end
	if #okeys > 0 then
		local labelled = { }
		for i, k in ipairs(okeys) do labelled[i] = { keyLabel(ctx, k), k } end
		sort(labelled, function(a, b) return a[1] < b[1] end)
		for i, e in ipairs(labelled) do
			list[#list + 1] = { "kkey", nil, path .. "[@" .. i .. ":key]", e[2] }
			list[#list + 1] = { "kval", nil, path .. "[@" .. i .. "]", rawget(c, e[2]) }
		end
	end
	if kind == "table" and c == t then
		local mt = getmetatable(t)
		if mt ~= nil then list[#list + 1] = { "meta", nil, path .. "<meta>", mt } end
	end
	return list
end

-- Pass 1: canonical paths and reference counts.
local function walk(ctx, root)
	local canon, count, isMeta = ctx.canon, ctx.count, ctx.isMeta
	canon[root] = ""
	count[root] = 1
	local queue, head = { root }, 1
	while queue[head] do
		local t = queue[head]
		head = head + 1
		local kind, c, cls = classify(ctx, t)
		for _, m in ipairs(members(ctx, t, kind, c, cls, canon[t], isMeta[t])) do
			local v = m[4]
			if type(v) == "table" and hasIdentity((classify(ctx, v))) then
				count[v] = (count[v] or 0) + 1
				if canon[v] == nil then
					canon[v] = m[3]
					if m[1] == "meta" then isMeta[v] = true end
					queue[#queue + 1] = v
				end
			end
		end
	end
end

local function isActiveSkill(c)
	return rawget(c, "activeEffect") ~= nil and rawget(c, "skillFlags") ~= nil
end

local emitValue

-- ModDB mods grouped by name (names sorted), ModList mods in order.
local function emitStoreMods(ctx, t, c, cls)
	local at = S.modAt[t]
	if cls == "ModDB" then
		ctx:put("{")
		local mods = rawget(c, "mods")
		if type(mods) == "table" then
			local names = sortedKeys(mods)
			for i, name in ipairs(names) do
				if i > 1 then ctx:put(",") end
				ctx:put(keyJson(name) .. ":[")
				local list = rawget(mods, name)
				local j = 1
				while type(list) == "table" do
					local m = rawget(list, j)
					if m == nil then break end
					if j > 1 then ctx:put(",") end
					if type(m) == "table" then encodeMod(ctx, m, at and at[m]) else encodePlain(ctx, m) end
					j = j + 1
				end
				ctx:put("]")
			end
		end
		ctx:put("}")
	else
		ctx:put("[")
		local j = 1
		while true do
			local m = rawget(c, j)
			if m == nil then break end
			if j > 1 then ctx:put(",") end
			if type(m) == "table" then encodeMod(ctx, m, at and at[m]) else encodePlain(ctx, m) end
			j = j + 1
		end
		ctx:put("]")
	end
end

local function emitTable(ctx, t, path)
	local kind, c, cls = classify(ctx, t)
	if kind == "marker" then ctx:put(c) return end
	if kind == "mod" then return encodeMod(ctx, c, nil) end
	-- an identity table: in full only at its canonical path
	if ctx.canon[t] ~= path or ctx.emitted[t] then
		local id = ctx.ids[t]
		if not id then
			id = ctx.nextId
			ctx.nextId = id + 1
			ctx.ids[t] = id
		end
		ctx:put('{"$ref":' .. id .. '}')
		return
	end
	ctx.emitted[t] = true
	local id
	if (ctx.count[t] or 1) > 1 then
		id = ctx.ids[t]
		if not id then
			id = ctx.nextId
			ctx.nextId = id + 1
			ctx.ids[t] = id
		end
	end
	local list = members(ctx, t, kind, c, cls, path, ctx.isMeta[t])
	local skill = kind == "table" and isActiveSkill(c)
	if kind == "table" and not id and not skill and #list > 0 then
		local array = true
		for _, m in ipairs(list) do
			if m[1] ~= "seq" then array = false break end
		end
		if array then
			ctx:put("[")
			for i, m in ipairs(list) do
				if i > 1 then ctx:put(",") end
				emitValue(ctx, m[4], m[3], m[5])
			end
			ctx:put("]")
			return
		end
	end
	ctx:put("{")
	local first = true
	local function sep() if first then first = false else ctx:put(",") end end
	if id then sep() ctx:put('"$id":' .. id) end
	if kind == "store" then
		sep()
		ctx:put('"$store":' .. quote(cls) .. ',"mods":')
		emitStoreMods(ctx, t, c, cls)
	elseif kind == "item" then
		sep()
		ctx:put('"$item":')
		encodeScalar(ctx, rawget(c, "name"))
		sep()
		ctx:put('"$modLines":')
		encodePlain(ctx, itemModLines(c))
	end
	if skill then
		local f = rawget(_G, "cacheSkillUUID")
		if type(f) == "function" and ctx.rootEnv then
			local ok, uuid = pcall(f, c, ctx.rootEnv)
			if ok and type(uuid) == "string" then
				sep()
				ctx:put('"$uuid":' .. jstr(uuid))
			end
		end
	end
	local open, groupFirst = nil, true
	local function close()
		if open == "seq" or open == "keys" then ctx:put("]")
		elseif open == "num" then ctx:put("}") end
		open = nil
	end
	local function openGroup(g, head)
		if open ~= g then
			close()
			sep()
			ctx:put(head)
			open = g
			groupFirst = true
		end
	end
	for _, m in ipairs(list) do
		local g = m[1]
		if g == "str" then
			close()
			sep()
			ctx:put(m[2] .. ":")
			emitValue(ctx, m[4], m[3], m[5])
		elseif g == "seq" then
			openGroup("seq", '"$seq":[')
			if not groupFirst then ctx:put(",") end
			groupFirst = false
			emitValue(ctx, m[4], m[3])
		elseif g == "num" then
			openGroup("num", '"$num":{')
			if not groupFirst then ctx:put(",") end
			groupFirst = false
			ctx:put(m[2] .. ":")
			emitValue(ctx, m[4], m[3])
		elseif g == "kkey" then
			openGroup("keys", '"$keys":[')
			if not groupFirst then ctx:put(",") end
			groupFirst = false
			ctx:put("[")
			emitValue(ctx, m[4], m[3])
			ctx:put(",")
		elseif g == "kval" then
			emitValue(ctx, m[4], m[3])
			ctx:put("]")
		elseif g == "meta" then
			close()
			sep()
			ctx:put('"$meta":')
			emitValue(ctx, m[4], m[3])
		end
	end
	close()
	ctx:put("}")
end

emitValue = function(ctx, v, path, withUp)
	local tv = type(v)
	if tv == "table" then return emitTable(ctx, v, path) end
	if tv == "function" then return encodeFn(ctx, v, withUp) end
	return encodeScalar(ctx, v)
end

---------------------------------------------------------------------------
-- Documents
---------------------------------------------------------------------------

-- doc = { build, hit, probe, n, kind, stack = { hit ids }, env, envId, locals }
function S.encodeDoc(doc)
	local env = doc.env
	local ctx = newCtx(env)
	ctx.canon, ctx.isMeta, ctx.emitted = { }, { }, { }
	prepare(ctx)
	local root = { }
	root.env = env
	root.locals = doc.locals or { }
	root.globalCache = rawget(_G, "GlobalCache")
	walk(ctx, root)
	local head = { }
	head[#head + 1] = '{"$env":' .. (doc.envId and quote(doc.envId) or "null")
	head[#head + 1] = '"build":' .. jstr(tostring(doc.build or ""))
	head[#head + 1] = '"format":"pob-state/2"'
	head[#head + 1] = '"hit":' .. jstr(doc.hit)
	head[#head + 1] = '"kind":' .. quote(doc.kind)
	head[#head + 1] = '"n":' .. jnum(doc.n)
	head[#head + 1] = '"probe":' .. jstr(doc.probe)
	-- the probe process (one id per process; CCR-0067 §3)
	if __probe_cfg and type(__probe_cfg.runId) == "string" then
		head[#head + 1] = '"run":' .. jstr(__probe_cfg.runId)
	end
	head[#head + 1] = '"root":'
	ctx:put(concat(head, ","))
	emitValue(ctx, root, "")
	local stack = { }
	for i, h in ipairs(doc.stack or { }) do stack[i] = jstr(h) end
	ctx:put(',"stack":[' .. concat(stack, ",") .. "]}")
	return concat(ctx.buf)
end

-- An empty list for S.json (`[]`; a plain empty table is written `{}`).
S.EMPTY_ARRAY = S.EMPTY_ARRAY or { }

-- `t` as a JSON list for S.json, `[]` when empty.
function S.array(t)
	if t == nil or next(t) == nil then return S.EMPTY_ARRAY end
	return t
end

-- Plain JSON for small Lua values (probe index files): sorted keys,
-- sequences as arrays.
function S.json(v)
	local ctx = newCtx(nil)
	ctx.plainOnly = true
	encodePlain(ctx, v)
	return concat(ctx.buf)
end
