-- pob-oracle: write provenance (calc-architecture.md §9.2).
--
-- Loaded before PoB boots, after state_dump.lua and only when provenance is
-- enabled. The patcher rewrites the creation sites of `actor.output`,
-- `output.MainHand/OffHand/Minion`, `minion.output` and the breakdown table
-- to `__PROV(kind, <table>)`, which returns an empty proxy whose
-- `__newindex` records the last writer `file:line` of every key and stores
-- the value in a backing table; `__index` reads from the backing table.
-- Lua 5.1 / LuaJIT (without 5.2 compat) ignore `__pairs`, so `pairs`,
-- `next` and `ipairs` are replaced here, before any PoB module captures
-- them, with versions that iterate a proxy's backing table. Everything else
-- (string keys, reads, writes, deletes, copyTable, wipeTable) behaves as on
-- the plain table. `#proxy` is 0, which no proxied table relies on (they
-- are string-keyed).
--
-- The patcher also appends `if __PROV_MOD then __PROV_MOD(self, mod) end`
-- to every statement of Classes/ModDB.lua and Classes/ModList.lua that
-- puts a mod into a store; it records, per store, the first stack frame
-- outside ModDB/ModList/ModStore that caused the insert.

local S = __state_dump
local type, rawset, setmetatable, tostring = type, rawset, setmetatable, tostring
local rawpairs, rawnext, rawipairs = pairs, next, ipairs
local proxyStore = S.proxyStore
local where = S.where

-- proxy -> { key -> "file:line" }, proxy -> kind
local writers = setmetatable({ }, { __mode = "k" })
local kinds = setmetatable({ }, { __mode = "k" })
S.writers = writers
S.proxyKind = kinds

local proxyMeta = {
	__index = function(t, k)
		local s = proxyStore[t]
		if s then return s[k] end
		return nil
	end,
	__newindex = function(t, k, v)
		local s = proxyStore[t]
		if not s then
			-- a copy that inherited this metatable (copyTableSafe preserveMeta)
			rawset(t, k, v)
			return
		end
		s[k] = v
		if type(v) ~= "function" then
			-- level 1 = this metamethod, 2 = the assigning code
			local pos = where(2)
			writers[t][k] = pos ~= "" and pos or nil
		end
	end,
}
S.proxyMeta = proxyMeta

function __PROV(kind, t)
	if type(t) ~= "table" or proxyStore[t] then return t end
	local p = setmetatable({ }, proxyMeta)
	proxyStore[p] = t
	local w = { }
	for k in rawpairs(t) do w[k] = "pre:" .. kind end
	writers[p] = w
	kinds[p] = kind
	return p
end

function pairs(t)
	local s = proxyStore[t]
	if s then return rawnext, s, nil end
	return rawpairs(t)
end

function next(t, k)
	local s = proxyStore[t]
	if s then return rawnext(s, k) end
	return rawnext(t, k)
end

function ipairs(t)
	local s = proxyStore[t]
	if s then return rawipairs(s) end
	return rawipairs(t)
end

-- Mod insertion provenance.
local skipFiles = {
	["Classes/ModDB.lua"] = true,
	["Classes/ModList.lua"] = true,
	["Classes/ModStore.lua"] = true,
}
local modAt = S.modAt
local weakKeys = { __mode = "k" }

function __PROV_MOD(store, mod)
	if type(mod) ~= "table" then return end
	-- k = 1 is the ModDB/ModList method that stored the mod
	for k = 2, 16 do
		local pos = where(k)
		if pos ~= "" then
			local file = pos:match("^(.*):%d+$")
			if not skipFiles[file] and not (file and file:find('^%[string "pre"%]')) then
				local at = modAt[store]
				if not at then
					at = setmetatable({ }, weakKeys)
					modAt[store] = at
				end
				at[mod] = pos
				return
			end
		end
	end
end

-- Flattened last writers of an output tree: nested proxies by key, other
-- nested tables inherit the writer of the key that holds them (the leaf
-- paths of diff.py / pob_calc_diff: `MainHand.Speed`, `SkillDPS.1.dps`).
local function flattenWriters(p, prefix, out, seen)
	local s = proxyStore[p]
	if not s or seen[p] then return end
	seen[p] = true
	local w = writers[p] or { }
	for k, v in rawpairs(s) do
		if type(k) == "string" or type(k) == "number" then
			local path = prefix .. tostring(k)
			if proxyStore[v] then
				out[path] = w[k]
				flattenWriters(v, path .. ".", out, seen)
			else
				out[path] = w[k]
			end
		end
	end
end

-- { output = {path -> writer}, enemy = ..., minion = ..., breakdown = ... }
-- for an env (normally calcsTab.mainEnv).
function S.provenanceOf(env)
	local doc = { }
	if type(env) ~= "table" then return doc end
	for _, spec in ipairs({
		{ "output", env.player and rawget(env.player, "output") },
		{ "breakdown", env.player and rawget(env.player, "breakdown") },
		{ "enemy", env.enemy and rawget(env.enemy, "output") },
		{ "minion", env.minion and rawget(env.minion, "output") },
		{ "minionBreakdown", env.minion and rawget(env.minion, "breakdown") },
	}) do
		local p = spec[2]
		if p and proxyStore[p] then
			local out = { }
			flattenWriters(p, "", out, { })
			doc[spec[1]] = out
		end
	end
	return doc
end
