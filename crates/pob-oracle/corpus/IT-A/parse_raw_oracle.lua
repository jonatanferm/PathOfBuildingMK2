-- IT-A oracle (crates/pob-oracle/tests/IT-A.rs): runs inside a booted PoB (LuaJIT, jit.off).
--
-- Records, for every `Item:ParseRaw` call it makes, everything the Rust port needs to replay and
-- check that call:
--   * the call's arguments (raw, rarity, highQuality) and the item set-up before it;
--   * every call ParseRaw makes into code other units own, in order, with its result:
--       "A" itemLib.applyRange (IT-C), "P" modLib.parseMod (MP0-MP6), "C" Item:CanHaveMod (IT-C)
--     (calls made by BuildModList itself are not recorded: BuildModList is IT-C's);
--   * the item's state at the entry of BuildModList (Item.lua:1635), i.e. every field ParseRaw
--     set, and what ParseRaw reads after BuildModList (jewelData.radiusIndex) and sets after it
--     (jewelRadiusIndex, isUnique).
--
-- Inputs (globals set by the test's prologue):
--   __ita_out    = path of the JSONL output file (one item record per line)
--   __ita_corpus = { { src = "<build>#<n>", raw = "<item text>", id = n|nil, variant = n|nil,
--                      variantAlt = n|nil, ..., variantAlt5 = n|nil }, ... }
--   __ita_synth  = { "<item text>", ... }   synthetic items (parsed as pasted: rarity nil)
--   __ita_limit  = max uniques (nil = all)
-- Returns a JSON header (counts, data.jewelRadius labels, main.defaultItemAffixQuality).

new("Item"):Item() -- loads the class
local ItemClass = common.classes.Item

-- ---- JSON: generic state encoder ($t = list of [key, value]; keys keep their Lua type) ------
local function encstr(s)
	return '"' .. s:gsub('[%c"\\\128-\255]', function(c)
		if c == '"' then return '\\"' elseif c == "\\" then return "\\\\" end
		return string.format("\\u%04x", c:byte())
	end) .. '"'
end
-- strings are raw bytes: bytes >= 128 are written as \u00XX and decoded back to bytes by the test
local function encnum(v)
	if v ~= v then return '"$nan"' elseif v == math.huge then return '"$inf"' elseif v == -math.huge then return '"$-inf"' end
	return string.format("%.17g", v)
end
local function enckey(k)
	local t = type(k)
	if t == "string" then return encstr(k) elseif t == "number" then return encnum(k) elseif t == "boolean" then return k and '{"$b":true}' or '{"$b":false}' end
	return '"$badkey"'
end

-- ---- JSON: pob-dump shape (crates/pob-dump/FORMAT.md) for mods, so serde builds pob_gamedata::Mod
local function dnum(v)
	if v ~= v then return '{"$float":"nan"}' end
	if v == math.huge then return '{"$float":"inf"}' elseif v == -math.huge then return '{"$float":"-inf"}' end
	if v == 0 and 1 / v < 0 then return '{"$float":"-0"}' end
	if v == math.floor(v) and math.abs(v) < 2^53 then return string.format("%d", v) end
	return string.format("%.17g", v)
end
local denc
denc = function(v, out)
	local t = type(v)
	if t == "nil" then out[#out + 1] = "null"
	elseif t == "boolean" then out[#out + 1] = v and "true" or "false"
	elseif t == "number" then out[#out + 1] = dnum(v)
	elseif t == "string" then out[#out + 1] = encstr(v)
	elseif t == "function" then out[#out + 1] = '{"$fn":"?","src":"?"}'
	elseif t == "table" then
		local n = 0
		while rawget(v, n + 1) ~= nil do n = n + 1 end
		local count = 0
		for _ in pairs(v) do count = count + 1 end
		if n >= 1 and count == n and getmetatable(v) == nil then
			out[#out + 1] = "["
			for i = 1, n do
				if i > 1 then out[#out + 1] = "," end
				denc(v[i], out)
			end
			out[#out + 1] = "]"
			return
		end
		local skeys, nkeys = { }, { }
		for k in pairs(v) do
			if type(k) == "string" then skeys[#skeys + 1] = k
			elseif type(k) == "number" and not (k >= 1 and k <= n and k == math.floor(k)) then nkeys[#nkeys + 1] = k end
		end
		table.sort(skeys)
		table.sort(nkeys)
		out[#out + 1] = "{"
		local first = true
		local function sep() if not first then out[#out + 1] = "," end first = false end
		for _, k in ipairs(skeys) do
			sep()
			out[#out + 1] = encstr(k:sub(1, 1) == "$" and "$" .. k or k) .. ":"
			denc(v[k], out)
		end
		if n >= 1 then
			sep()
			out[#out + 1] = '"$seq":['
			for i = 1, n do
				if i > 1 then out[#out + 1] = "," end
				denc(v[i], out)
			end
			out[#out + 1] = "]"
		end
		if #nkeys > 0 then
			sep()
			out[#out + 1] = '"$num":{'
			for i, k in ipairs(nkeys) do
				if i > 1 then out[#out + 1] = "," end
				out[#out + 1] = encstr(string.format("%.17g", k)) .. ":"
				denc(v[k], out)
			end
			out[#out + 1] = "}"
		end
		out[#out + 1] = "}"
	else
		out[#out + 1] = "null"
	end
end
local function encodeMods(modList)
	if modList == nil then return "null" end
	local out = { "[" }
	for i, m in ipairs(modList) do
		if i > 1 then out[#out + 1] = "," end
		denc(m, out)
	end
	out[#out + 1] = "]"
	return table.concat(out)
end

-- ---- recording state ------------------------------------------------------------------------
local cur          -- the parse record being filled (nil = not recording)
local curItem      -- the item whose ParseRaw is being recorded
local modsCall = setmetatable({ }, { __mode = "k" }) -- modList table -> index of its "P" call

local function repr(x)
	local t = type(x)
	if t == "nil" then return "nil"
	elseif t == "number" then return string.format("%.17g", x)
	elseif t == "table" then
		local p = { }
		for i, v in ipairs(x) do p[i] = repr(v) end
		return "{" .. table.concat(p, ",") .. "}"
	end
	return tostring(x)
end
local function recording()
	return cur and not cur.suspended
end

-- applyRange's own `modLib.parseMod` calls (ItemTools.lua:323, its fallback path) belong to
-- IT-C's applyRange, which the Rust side replays as a whole: they are not recorded.
local inApplyRange = 0
local origApplyRange = itemLib.applyRange
itemLib.applyRange = function(line, range, valueScalar, baseValueScalar)
	inApplyRange = inApplyRange + 1
	local ok, r = pcall(origApplyRange, line, range, valueScalar, baseValueScalar)
	inApplyRange = inApplyRange - 1
	if not ok then error(r, 0) end
	if recording() then
		local key = line .. "\1" .. repr(range) .. "\1" .. repr(valueScalar) .. "\1" .. repr(baseValueScalar)
		cur.calls[#cur.calls + 1] = "[" .. encstr("A") .. "," .. encstr(key) .. "," .. encstr(r) .. "]"
	end
	return r
end

local origParseMod = modLib.parseMod
modLib.parseMod = function(line, isComb)
	local modList, extra = origParseMod(line, isComb)
	if recording() and inApplyRange == 0 then
		cur.calls[#cur.calls + 1] = "[" .. encstr("P") .. "," .. encstr(line) .. "," .. (isComb and "true" or "false")
			.. "," .. encodeMods(modList) .. "," .. (extra and encstr(extra) or "null") .. "]"
		if modList then modsCall[modList] = #cur.calls end
	end
	return modList, extra
end

local origCanHaveMod = ItemClass.CanHaveMod
ItemClass.CanHaveMod = function(self, mod, includeTags)
	local r = origCanHaveMod(self, mod, includeTags)
	if recording() and self == curItem then
		local id = "?"
		for k, v in pairs(self.affixes or { }) do
			if v == mod then id = k break end
		end
		cur.calls[#cur.calls + 1] = "[" .. encstr("C") .. "," .. encstr(id) .. "," .. (r and "true" or "false") .. "]"
	end
	return r
end

-- ---- item state ----------------------------------------------------------------------------
local baseKeyOf = { }
for k, v in pairs(data.itemBases) do baseKeyOf[v] = k end
local affixKeyOf = { }
for k, v in pairs(data.itemMods) do affixKeyOf[v] = "itemMods:" .. k end
local rareKeyOf = { }
for k, v in pairs(data.rareLikeUniques) do
	affixKeyOf[v.affixes] = affixKeyOf[v.affixes] or ("rareLike:" .. k)
	rareKeyOf[v] = k
end
local enchKeyOf = { }
for k, v in pairs(data.enchantments) do enchKeyOf[v] = enchKeyOf[v] or k end
local foulKeyOf = { }
for k, v in pairs(data.foulbornMap or { }) do foulKeyOf[v] = k end
local clusterKeyOf = { }
for k, v in pairs(data.clusterJewels and data.clusterJewels.jewels or { }) do clusterKeyOf[v] = k end

-- Fields that only BuildModList (IT-C) or the UI set: never part of ParseRaw's state.
local skip = {
	Object = true, modList = true, slotModList = true, baseModList = true, buffModList = true,
	weaponData = true, flaskData = true, tinctureData = true, jewelData = true, jewelRadiusData = true,
	grantedSkills = true, modSource = true, rangeLineList = true, selectableSocketCount = true,
	socketedJewelEffectModifier = true, hasModTags = true,
}

local enc
enc = function(v, out, depth, key)
	local t = type(v)
	if t == "nil" then out[#out + 1] = "null"
	elseif t == "boolean" then out[#out + 1] = v and "true" or "false"
	elseif t == "number" then out[#out + 1] = encnum(v)
	elseif t == "string" then out[#out + 1] = encstr(v)
	elseif t == "table" then
		if key == "modList" then
			local idx = modsCall[v]
			if idx then
				out[#out + 1] = '{"$modsOf":' .. idx .. '}'
			else
				out[#out + 1] = '{"$mods":' .. encodeMods(v) .. '}'
			end
			return
		end
		if key == "table" and curItem and (v == curItem.prefixes or v == curItem.suffixes) then
			out[#out + 1] = v == curItem.prefixes and '{"$ref":"prefixes"}' or '{"$ref":"suffixes"}'
			return
		end
		if getmetatable(v) ~= nil then out[#out + 1] = '{"$obj":true}' return end
		if depth > 12 then out[#out + 1] = '{"$deep":true}' return end
		out[#out + 1] = '{"$t":['
		local first = true
		local n = #v
		local seen = { }
		for i = 1, n do
			if v[i] ~= nil then
				seen[i] = true
				if not first then out[#out + 1] = "," end
				first = false
				out[#out + 1] = "[" .. encnum(i) .. ","
				enc(v[i], out, depth + 1, nil)
				out[#out + 1] = "]"
			end
		end
		for k, x in pairs(v) do
			if not seen[k] then
				if not first then out[#out + 1] = "," end
				first = false
				out[#out + 1] = "[" .. enckey(k) .. ","
				enc(x, out, depth + 1, type(k) == "string" and k or nil)
				out[#out + 1] = "]"
			end
		end
		out[#out + 1] = "]}"
	else
		out[#out + 1] = '{"$fn":true}'
	end
end
local function json(v, key)
	local out = { }
	enc(v, out, 0, key)
	return table.concat(out)
end
local function ref(kind, key)
	return '{"$ref":' .. encstr(kind .. ":" .. tostring(key)) .. '}'
end

local function dumpState(item)
	local parts = { }
	local keys = { }
	for k in pairs(item) do
		if type(k) == "string" then keys[#keys + 1] = k end
	end
	table.sort(keys)
	for _, k in ipairs(keys) do
		local v = rawget(item, k)
		if type(v) ~= "function" and not skip[k] then
			local e
			if k == "base" then e = ref("base", baseKeyOf[v] or "$unknown")
			elseif k == "affixes" then e = '{"$ref":' .. encstr(affixKeyOf[v] or "$unknown") .. '}'
			elseif k == "rareLikeUnique" then e = ref("rareLike", rareKeyOf[v] or "$unknown")
			elseif k == "enchantments" then e = ref("ench", enchKeyOf[v] or "$unknown")
			elseif k == "mutatedLines" then e = ref("foulborn", foulKeyOf[v] or "$unknown")
			elseif k == "clusterJewel" then e = ref("cluster", clusterKeyOf[v] or "$unknown")
			else e = json(v, k) end
			parts[#parts + 1] = encstr(k) .. ":" .. e
		end
	end
	return "{" .. table.concat(parts, ",") .. "}"
end

local origBuildModList = ItemClass.BuildModList
ItemClass.BuildModList = function(self)
	if recording() and self == curItem then
		cur.state = dumpState(self)
		cur.suspended = true
		local ok, err = pcall(origBuildModList, self)
		cur.suspended = false
		cur.bml = '{"hasJewelData":' .. (self.jewelData and "true" or "false")
			.. ',"radiusIndex":' .. (self.jewelData and self.jewelData.radiusIndex and encnum(self.jewelData.radiusIndex) or "null")
			.. ',"err":' .. (ok and "null" or encstr(tostring(err))) .. '}'
		if not ok then error(err, 0) end
		return
	end
	return origBuildModList(self)
end

-- ---- driver ----------------------------------------------------------------------------------
local outFile = assert(io.open(__ita_out, "w"))
local nItems, nParses = 0, 0

local setupKeys = { "id", "variant", "variantAlt", "variantAlt2", "variantAlt3", "variantAlt4", "variantAlt5" }
local function setupJson(setup)
	if not setup then return "null" end
	local parts = { }
	for _, k in ipairs(setupKeys) do
		if setup[k] ~= nil then parts[#parts + 1] = encstr(k) .. ":" .. encnum(setup[k]) end
	end
	return "{" .. table.concat(parts, ",") .. "}"
end

-- One recorded ParseRaw call on `item`; `setup` fields are applied first (ItemsTab.lua:1224-1243).
local function doParse(item, raw, rarity, highQuality, setup)
	if setup then
		item.id = setup.id
		item.variant = setup.variant
		for i, k in ipairs({ "variantAlt", "variantAlt2", "variantAlt3", "variantAlt4", "variantAlt5" }) do
			if setup[k] then
				item[i == 1 and "hasAltVariant" or ("hasAltVariant" .. i)] = true
				item[k] = setup[k]
			end
		end
	end
	local p = { calls = { } }
	cur, curItem = p, item
	local ok, err = pcall(item.ParseRaw, item, raw, rarity, highQuality)
	cur = nil
	nParses = nParses + 1
	local after = '{"jewelRadiusIndex":' .. (item.jewelRadiusIndex and encnum(item.jewelRadiusIndex) or "null")
		.. ',"isUnique":' .. (item.isUnique == nil and "null" or (item.isUnique and "true" or "false")) .. '}'
	local s = "{" .. '"raw":' .. encstr(raw) .. ',"rarity":' .. (rarity and encstr(rarity) or "null")
		.. ',"highQuality":' .. (highQuality and "true" or "false") .. ',"setup":' .. setupJson(setup)
		.. ',"calls":[' .. table.concat(p.calls, ",") .. ']'
		.. ',"state":' .. (p.state or "null") .. ',"bml":' .. (p.bml or "null")
		.. ',"after":' .. after .. ',"err":' .. (ok and "null" or encstr(tostring(err))) .. "}"
	curItem = nil
	return s, ok
end

local function emit(src, kind, parses)
	outFile:write('{"src":', encstr(src), ',"kind":', encstr(kind), ',"parses":[', table.concat(parses, ","), "]}\n")
	nItems = nItems + 1
end

local counts = { }
local function count(kind) counts[kind] = (counts[kind] or 0) + 1 end

-- uniques (Main.lua:164-166), every item type, in key order; then every variant of each, as the
-- items tab re-parses it (BuildRaw with `variant` set, parsed into a fresh item, as pasted)
local types = { }
for t in pairs(data.uniques) do types[#types + 1] = t end
table.sort(types)
local nUniques = 0
for _, t in ipairs(types) do
	for i, raw in ipairs(data.uniques[t]) do
		if __ita_limit and nUniques >= __ita_limit then break end
		nUniques = nUniques + 1
		local item = new("Item"):Item()
		local s, ok = doParse(item, sanitiseText(raw), "UNIQUE", true)
		emit("unique:" .. t .. "#" .. i, "unique", { s })
		count("unique")
		if ok and item.variantList and #item.variantList > 1 then
			for v = 1, #item.variantList do
				item.variant = v
				local okR, text = pcall(item.BuildRaw, item)
				if okR then
					local item2 = new("Item"):Item()
					local s2 = doParse(item2, text, nil, false)
					emit("unique:" .. t .. "#" .. i .. "@" .. v, "variant", { s2 })
					count("variant")
				end
			end
		end
	end
end

-- rares (Main.lua:178)
for i, raw in ipairs(data.rares) do
	local item = new("Item"):Item()
	local s = doParse(item, sanitiseText(raw), "RARE", true)
	emit("rare#" .. i, "rare", { s })
	count("rare")
end

-- build corpus items, as ItemsTab:Load parses them (ItemsTab.lua:1223-1246): `Item("")`, then the
-- id/variant attributes, then `ParseRaw(text)` on the same object (no sanitiseText)
for _, entry in ipairs(__ita_corpus or { }) do
	local item = new("Item"):Item()
	local s1 = doParse(item, sanitiseText(""), nil, false)
	local s2 = doParse(item, entry.raw, nil, false, entry)
	emit(entry.src, "corpus", { s1, s2 })
	count("corpus")
end

-- synthetic items (pasted text: ItemsTab.lua:1949 `new("Item"):Item(itemRaw)`)
for i, raw in ipairs(__ita_synth or { }) do
	local item = new("Item"):Item()
	local s = doParse(item, sanitiseText(raw), nil, false)
	emit("synthetic#" .. i, "synthetic", { s })
	count("synthetic")
end

outFile:close()

local labels = { }
for i, r in ipairs(data.jewelRadius or { }) do labels[i] = encstr(r.label) end
local cparts = { }
for k, n in pairs(counts) do cparts[#cparts + 1] = encstr(k) .. ":" .. n end
return '{"items":' .. nItems .. ',"parses":' .. nParses .. ',"counts":{' .. table.concat(cparts, ",") .. '}'
	.. ',"jewelRadius":[' .. table.concat(labels, ",") .. ']'
	.. ',"defaultItemAffixQuality":' .. (main and main.defaultItemAffixQuality and encnum(main.defaultItemAffixQuality) or "null")
	.. '}'
