-- IT-B oracle (crates/pob-oracle/tests/IT-B.rs): runs inside a booted PoB (LuaJIT, jit.off).
-- Inputs (globals set by the test's prologue):
--   __itb_corpus   = { { src = "<build>#<n>", raw = "<item text>" }, ... }  (items of the build corpus)
--   __itb_stringify = { <table>, ... }                                       (Utils.stringify vectors)
--   __itb_tmp      = path of a scratch file for Utils.saveTableToFile
--   __itb_limit    = max uniques (nil = all)
-- Returns one JSON document (see the test for its shape).

new("Item") -- loads the class
local ItemClass = common.classes.Item
local utils = LoadModule("Modules/Utils")

-- ---- JSON encoder (keys keep their Lua type; tables keep their pairs order) ----------------
local function encstr(s)
	return '"' .. s:gsub('[%c"\\]', function(c)
		if c == '"' then return '\\"' elseif c == "\\" then return "\\\\" end
		return string.format("\\u%04x", c:byte())
	end) .. '"'
end
local function encnum(v)
	if v ~= v then return '"$nan"' elseif v == math.huge then return '"$inf"' elseif v == -math.huge then return '"$-inf"' end
	return string.format("%.17g", v)
end
local enc
local function enckey(k)
	local t = type(k)
	if t == "string" then return encstr(k) elseif t == "number" then return encnum(k) elseif t == "boolean" then return k and '{"$b":true}' or '{"$b":false}' end
	return '"$badkey"'
end
enc = function(v, out, depth)
	local t = type(v)
	if t == "nil" then out[#out + 1] = "null"
	elseif t == "boolean" then out[#out + 1] = v and "true" or "false"
	elseif t == "number" then out[#out + 1] = encnum(v)
	elseif t == "string" then out[#out + 1] = encstr(v)
	elseif t == "table" then
		if getmetatable(v) ~= nil then out[#out + 1] = '{"$obj":true}' return end
		if depth > 10 then out[#out + 1] = '{"$deep":true}' return end
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
				enc(v[i], out, depth + 1)
				out[#out + 1] = "]"
			end
		end
		for k, x in pairs(v) do
			if not seen[k] then
				if not first then out[#out + 1] = "," end
				first = false
				out[#out + 1] = "[" .. enckey(k) .. ","
				if k == "modList" then
					-- a mod line's parsed mods: not read by the functions under test (size)
					out[#out + 1] = '{"$truthy":true}'
				else
					enc(x, out, depth + 1)
				end
				out[#out + 1] = "]"
			end
		end
		out[#out + 1] = "]}"
	else
		out[#out + 1] = '{"$fn":true}'
	end
end
local function json(v)
	local out = { }
	enc(v, out, 0)
	return table.concat(out)
end
-- A JSON array of already-encoded values.
local function jarr(list)
	return "[" .. table.concat(list, ",") .. "]"
end
local function jobj(pairsList)
	local parts = { }
	for _, kv in ipairs(pairsList) do
		parts[#parts + 1] = encstr(kv[1]) .. ":" .. kv[2]
	end
	return "{" .. table.concat(parts, ",") .. "}"
end

-- ---- item state dump ------------------------------------------------------------------------
local baseKeyOf = { }
for k, v in pairs(data.itemBases) do baseKeyOf[v] = k end
local affixKeyOf = { }
for k, v in pairs(data.itemMods) do affixKeyOf[v] = "itemMods:" .. k end
for k, v in pairs(data.rareLikeUniques) do affixKeyOf[v.affixes] = "rareLike:" .. k end

local marker = {
	base = true, affixes = true, rareLikeUnique = true, clusterJewel = true, enchantments = true,
	mutatedLines = true, jewelData = true, jewelRadiusData = true, modList = true, slotModList = true,
	baseModList = true, buffModList = true, weaponData = true, flaskData = true, tinctureData = true,
	grantedSkills = true, pendingAffixList = true,
}
local function dumpState(item)
	local parts = { }
	for k, v in pairs(item) do
		if type(k) == "string" and type(v) ~= "function" then
			local e
			if k == "base" then
				e = encstr(baseKeyOf[v] or "$unknown")
			elseif k == "affixes" then
				e = encstr(affixKeyOf[v] or "$unknown")
			elseif marker[k] then
				e = '{"$truthy":true}'
			else
				e = json(v)
			end
			parts[#parts + 1] = { k, e }
		end
	end
	return jobj(parts)
end

-- ---- per-item outputs -----------------------------------------------------------------------
local findSubstrings = { "life", "mana", "energy shield", "armour", "evasion", "fire", "cold", "lightning",
	"chaos", "physical", "elemental", "attack", "spell", "critical", "speed", "resistance", "attribute",
	"strength", "dexterity", "intelligence", "minion", "aura", "curse", "damage", "explicit life",
	"explicit fire", "explicit resistance", "explicit attack", "gem", "flask", "charges", "block" }
local lists = { "enchantModLines", "scourgeModLines", "classRequirementModLines", "implicitModLines", "explicitModLines", "crucibleModLines" }

local function pc(f, ...)
	local ok, r = pcall(f, ...)
	if ok then return json(r) end
	return jobj({ { "$err", encstr(tostring(r)) } })
end

local function basicOut(item)
	local o = { }
	o[#o + 1] = { "buildRaw", pc(item.BuildRaw, item) }
	local okSlot, slot = pcall(item.GetPrimarySlot, item)
	o[#o + 1] = { "primarySlot", okSlot and json(slot) or jobj({ { "$err", encstr(tostring(slot)) } }) }
	local cmv = { }
	for _, listName in ipairs(lists) do
		local rows = { }
		for _, modLine in ipairs(item[listName] or { }) do
			local ok1, a = pcall(item.CheckModLineVariant, item, modLine)
			local ok2, b = pcall(item.GetModLineVariantCount, item, modLine)
			rows[#rows + 1] = jarr({ ok1 and json(a and true or false) or '"$err"', ok2 and json(b) or '"$err"' })
		end
		cmv[#cmv + 1] = jarr(rows)
	end
	o[#o + 1] = { "cmv", jarr(cmv) }
	local find = { }
	local slotLower = okSlot and type(slot) == "string" and slot:lower() or ""
	for _, s in ipairs(findSubstrings) do
		local ok, r = pcall(item.FindModifierSubstring, item, s, slotLower)
		find[#find + 1] = ok and json(r and true or false) or '"$err"'
	end
	o[#o + 1] = { "find", jarr(find) }
	o[#o + 1] = { "findSlot", encstr(slotLower) }
	local groups = { }
	if item.variantGroups then
		for groupId in pairsSortByKey(item.variantGroups) do
			local elig = { }
			for v = 1, #(item.variantList or { }) do
				elig[#elig + 1] = json(item:IsVariantGroupOptionEligible(groupId, v) and true or false)
			end
			groups[#groups + 1] = jarr({ json(groupId), pc(item.GetVariantGroupOptions, item, groupId, false), pc(item.GetVariantGroupOptions, item, groupId, true), jarr(elig) })
		end
	end
	o[#o + 1] = { "groups", jarr(groups) }
	return o
end

-- Spawn weights, as compact strings (one entry per mod, "E" = a Lua error) over the mod ids in
-- bytewise order: the plain call over every `itemMods.Item` mod, the include/exclude call over a
-- sample (`(#id + id:byte(1)) % 5 == 0`), the necropolis weight over every `necropolisMods` mod.
local function sortedIds(t)
	local ids = { }
	for id in pairs(t) do ids[#ids + 1] = id end
	table.sort(ids)
	return ids
end
local itemModIds = sortedIds(data.itemMods.Item)
local necroModIds = sortedIds(data.necropolisMods)
local function sampled(modId)
	return (#modId + modId:byte(1)) % 5 == 0
end
local function wenc(ok, w)
	if not ok then return "E" end
	return encnum(w)
end
local function weightsOut(item, o)
	local w1, w2, w3 = { }, { }, { }
	for _, modId in ipairs(itemModIds) do
		local mod = data.itemMods.Item[modId]
		w1[#w1 + 1] = wenc(pcall(item.GetModSpawnWeight, item, mod))
		if sampled(modId) then
			w2[#w2 + 1] = wenc(pcall(item.GetModSpawnWeight, item, mod, { ["two_hand_weapon"] = true }, { ["one_hand_weapon"] = true }))
		end
	end
	for _, modId in ipairs(necroModIds) do
		w3[#w3 + 1] = wenc(pcall(item.GetNecropolisModSpawnWeight, item, data.necropolisMods[modId]))
	end
	o[#o + 1] = { "spawn", encstr(table.concat(w1, ",")) }
	o[#o + 1] = { "spawnIncl", encstr(table.concat(w2, ",")) }
	o[#o + 1] = { "necro", encstr(table.concat(w3, ",")) }
end

-- The mutation sequence the Rust test replays: NormaliseQuality, NormaliseVariantSelections,
-- every variant's BuildRaw (non-group variant items), then MutateMod on the first mutable line.
local function mutateOut(item, o)
	local ok = pcall(item.NormaliseQuality, item)
	o[#o + 1] = { "quality", ok and json(item.quality) or '"$err"' }
	ok = pcall(item.NormaliseVariantSelections, item)
	o[#o + 1] = { "nvs", ok and jobj({ { "selectedVersion", json(item.selectedVersion) }, { "variantGroupSelections", json(item.variantGroupSelections) } }) or '"$err"' }
	local variants = { }
	if item.variantList and not item.usesVariantGroups then
		local saved = item.variant
		for v = 1, #item.variantList do
			item.variant = v
			local rows = { }
			for _, modLine in ipairs(item.explicitModLines) do
				rows[#rows + 1] = json(item:CheckModLineVariant(modLine) and true or false)
			end
			-- the full text for the first 8 variants and the last (the text is large for items with many variants)
			local text = (v <= 8 or v == #item.variantList) and pc(item.BuildRaw, item) or "null"
			variants[#variants + 1] = jarr({ text, jarr(rows) })
		end
		item.variant = saved
	end
	o[#o + 1] = { "variants", jarr(variants) }
	for i, modLine in ipairs(item.explicitModLines) do
		if modLine.modId and modLine.newModId and item:CheckModLineVariant(modLine) then
			local args = jarr({ encstr(modLine.modId), encstr(modLine.newModId), json(not modLine.mutated) })
			local okM, err = pcall(item.MutateMod, item, modLine.modId, modLine.newModId, not modLine.mutated)
			o[#o + 1] = { "mutate", jobj({ { "args", args }, { "raw", okM and json(item.raw) or jobj({ { "$err", encstr(tostring(err)) } }) } }) }
			break
		end
	end
end

local itemsJson = { }
local function addItem(src, kind, item, extra)
	local o = { { "src", encstr(src) }, { "kind", encstr(kind) }, { "state", dumpState(item) } }
	for _, kv in ipairs(basicOut(item)) do o[#o + 1] = kv end
	if extra then extra(item, o) end
	itemsJson[#itemsJson + 1] = jobj(o)
end

-- uniques (Main.lua:164-175), every item type, in key order
local nUniques = 0
local types = { }
for t in pairs(data.uniques) do types[#types + 1] = t end
table.sort(types)
for _, t in ipairs(types) do
	for i, raw in ipairs(data.uniques[t]) do
		if __itb_limit and nUniques >= __itb_limit then break end
		local ok, item = pcall(function() return new("Item"):Item(raw, "UNIQUE", true) end)
		if ok and item then
			nUniques = nUniques + 1
			addItem("unique:" .. t .. "#" .. i, "unique", item, mutateOut)
		end
	end
end

-- rares (Main.lua:178-193): state before Craft, raw after Craft
local function addRare(src, kind, raw, withWeights)
	local ok, item = pcall(function() return new("Item"):Item(raw, "RARE", true) end)
	if ok and item and item.base then
		if item.crafted then
			if item.base.implicit and #item.implicitModLines == 0 then
				-- Automatically add implicit
				local implicitIndex = 1
				for line in item.base.implicit:gmatch("[^\n]+") do
					table.insert(item.implicitModLines, { line = line, modTags = item.base.implicitModTypes and item.base.implicitModTypes[implicitIndex] or { } })
					implicitIndex = implicitIndex + 1
				end
			end
		end
		addItem(src, kind, item, function(it, o)
			if withWeights then weightsOut(it, o) end
			if it.crafted then
				local okC, err = pcall(it.Craft, it)
				o[#o + 1] = { "craft", okC and json(it.raw) or jobj({ { "$err", encstr(tostring(err)) } }) }
			end
		end)
	end
end
for i, raw in ipairs(data.rares) do
	addRare("rare#" .. i, "rare", raw, i % 2 == 1)
end

-- synthetic crafted rares: no data.rares entry has a multi-value `{range:a,b}` affix, which
-- ParseRaw stores as a list (Item.lua:803-809), BuildRaw writes with table.concat (1811/1815)
-- and Craft passes to applyRange as is (2084; per-range values, ItemTools.lua:99/104). These
-- cover a list range on single- and two-range lines, a hybrid (two-line) mod, a list shorter
-- than the line's ranges (`ranges[i] or 0.5`), combined stats (same statOrder) and
-- `round(range, 3)` of a number range.
local craftSynth = { [[
Synthetic Range Ring
Ruby Ring
Crafted: true
Prefix: {range:0.25,0.75}AddedColdDamage3
Prefix: {range:1,0}AddedColdDamage2
Prefix: {range:0.1234567}IncreasedLife7
Suffix: None
]], [[
Synthetic Range Sword
Corsair Sword
Crafted: true
Prefix: {range:0,1}LocalAddedPhysicalDamage3
Prefix: {fractured}{range:0.2,0.8}LocalIncreasedPhysicalDamagePercentAndAccuracyRating3
Prefix: {range:0.9,}LocalAddedColdDamage3
]] }
for i, raw in ipairs(craftSynth) do
	addRare("craftsynth#" .. i, "craftsynth", raw, false)
end

-- synthetic variant-group / version items: the one unique with groups (Zana's Ingenuity) under
-- many group selections, and a made-up item with versions, groups and versioned base lines
local zana
for _, raw in ipairs(data.uniques.ring) do
	if raw:find("Zana's Ingenuity", 1, true) then zana = raw end
end
local versioned = [[
Test Versioned
Prismatic Ring
Version: Pre 3.0
Version: Current
Variant: A
Variant: B
Variant: C
Variant: D
Implicits: 1
{version:1}+(8-10)% to all Elemental Resistances
{version:2}{variant:1}{group:1}10% increased maximum Life
{version:1,2}{variant:2}{group:1}20% increased maximum Mana
{variant:3}{group:2}Damage cannot be Reflected
{version:2}{variant:1,3}{group:2}{tags:attack,speed}5% increased Attack Speed
{variant:4}{group:3}{tags:resource}+10 to maximum Mana
{version:1}{variant:4}{group:3}{tags:resource}+20 to maximum Mana
{group:2}Line with a group but no variant
{range:0.1234567}+(20-40) to maximum Life
{corruptedRange:1.23456}+(10-20) to Strength
{variant:9}{group:1}Line naming an unknown variant
(5-15)% increased Attack Speed
]]
local synth = { }
local seedS = 777
local function rndS(n)
	seedS = (seedS * 1103515245 + 12345) % 2147483648
	return seedS % n
end
for case = 1, 60 do
	local base = case % 2 == 1 and zana or versioned
	if base then
		local lines = { }
		local nGroups = base == zana and 4 or 4
		local nVariants = base == zana and 13 or 5
		for g = 1, nGroups do
			local v = rndS(nVariants + 1)
			if v > 0 then lines[#lines + 1] = "Selected Variant Group: " .. g .. "=" .. v end
		end
		if base == versioned then
			local sv = rndS(4)
			if sv > 0 then lines[#lines + 1] = "Selected Version: " .. sv end
		end
		synth[#synth + 1] = base:gsub("Implicits:", table.concat(lines, "\n") .. (#lines > 0 and "\n" or "") .. "Implicits:")
	end
end
for i, raw in ipairs(synth) do
	local ok, item = pcall(function() return new("Item"):Item(raw, "UNIQUE", true) end)
	if ok and item then
		addItem("synthetic#" .. i, "synthetic", item, mutateOut)
	end
end

-- build corpus items
for i, entry in ipairs(__itb_corpus or { }) do
	local ok, item = pcall(function() return new("Item"):Item(entry.raw) end)
	if ok and item then
		addItem(entry.src, "corpus", item, function(it, o)
			if i % 20 == 1 then weightsOut(it, o) end
			mutateOut(it, o)
		end)
	end
end

-- ---- Item.lua locals ------------------------------------------------------------------------
-- The headless LuaJIT has no `debug` library, so Item.lua is loaded once more with a trailing
-- `return` of its file-level locals (the re-run re-registers the Item class, which nothing
-- below uses).
local itemSrc = io.open("Classes/Item.lua", "r"):read("*a")
local locals = assert(loadstring(itemSrc .. "\nreturn { normaliseModLine = normaliseModLine, specToNumber = specToNumber, getCatalystScalar = getCatalystScalar, sortCraftedModLines = sortCraftedModLines }", "@Classes/Item.lua"))()
local normaliseModLine = locals.normaliseModLine
local specToNumber = locals.specToNumber
local getCatalystScalar = locals.getCatalystScalar
local sortCraftedModLines = locals.sortCraftedModLines

local fnNormalise, fnSpec, fnCatalyst, fnSort = { }, { }, { }, { }
local lineSet = { }
for t, list in pairs(data.uniques) do
	for _, raw in ipairs(list) do
		for line in raw:gmatch("[^\n]+") do lineSet[#lineSet + 1] = line end
	end
end
for i, line in ipairs(lineSet) do
	fnNormalise[#fnNormalise + 1] = jarr({ encstr(line), encstr((normaliseModLine(line))) })
	local spec = line:match("^[%a %(%)]+:? ?(.*)$") or line
	fnSpec[#fnSpec + 1] = jarr({ encstr(spec), json(specToNumber(spec)) })
end
for _, s in ipairs({ "+12", "-3.5", "1.2.3", ".", "+", "20%", "  5", "abc", "7 (augmented)", "1e5", "0x10", "--1", "+.5" }) do
	fnSpec[#fnSpec + 1] = jarr({ encstr(s), json(specToNumber(s)) })
end
local tagSets = { { }, { "attack" }, { "life", "defences" }, { "caster", "speed" }, { "elemental_damage", "fire" },
	{ "jewellery_attribute" }, { "critical", "attack" }, { "resource" }, { "physical_damage" }, { "evasion" } }
local catalystIds = { 0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 2.5, "3" }
local qualities = { false, 0, 7, 20 }
for ti, tags in ipairs(tagSets) do
	for _, flags in ipairs({ { }, { prefix = true }, { suffix = true }, { unscalable = true } }) do
		local mod = { modTags = tags }
		for k, v in pairs(flags) do mod[k] = v end
		for _, c in ipairs(catalystIds) do
			for _, q in ipairs(qualities) do
				local r = getCatalystScalar(c, mod, q or nil)
				fnCatalyst[#fnCatalyst + 1] = jarr({ json(ti), json(mod.prefix or false), json(mod.suffix or false), json(mod.unscalable or false), json(c), json(q or nil), encnum(r) })
			end
		end
	end
end
-- sortCraftedModLines on pseudo-random lists (fixed LCG)
local seed = 12345
local function rnd(n)
	seed = (seed * 1103515245 + 12345) % 2147483648
	return seed % n
end
for case = 1, 400 do
	local list = { }
	local n = 1 + rnd(12)
	for i = 1, n do
		local m = { id = i }
		local g = rnd(5)
		if g == 0 then m.crafted = true elseif g == 1 then m.custom = true elseif g == 2 then m.fractured = true end
		if rnd(3) > 0 then m.order = rnd(6) end
		list[i] = m
	end
	local spec = { }
	for i, m in ipairs(list) do spec[i] = jarr({ json(m.crafted or false), json(m.custom or false), json(m.fractured or false), json(m.order) }) end
	local ok, err = pcall(sortCraftedModLines, list)
	local ids = { }
	for i, m in ipairs(list) do ids[i] = json(m.id) end
	fnSort[#fnSort + 1] = jarr({ jarr(spec), ok and jarr(ids) or '"$err"' })
end

-- ---- Utils ----------------------------------------------------------------------------------
local strOut = { }
for i, t in ipairs(__itb_stringify or { }) do
	strOut[#strOut + 1] = jarr({ pc(utils.stringify, t), pc(utils.stringify, t, true, 3), pc(utils.stringifyInline, t) })
end
local saveOut = '"$skip"'
if __itb_tmp and __itb_stringify and __itb_stringify[1] then
	local ok = pcall(utils.saveTableToFile, __itb_tmp, __itb_stringify[1], "Line one\nline two", false)
	if ok then
		local f = io.open(__itb_tmp, "r")
		saveOut = encstr(f:read("*a"))
		f:close()
	end
end
-- the open-error message (io.open's "<name>: <strerror>"), on a path in a missing directory
local saveErr = '"$skip"'
if __itb_tmp then
	local ok, err = pcall(utils.saveTableToFile, __itb_tmp .. ".missing/x.lua", { 1 }, nil, false)
	saveErr = ok and '"$noerr"' or encstr(tostring(err))
end

local delve = 0
for _, mod in pairs(data.itemMods.Item) do
	if ItemClass.CheckIfModIsDelve(nil, mod) then delve = delve + 1 end
end

return jobj({
	{ "items", jarr(itemsJson) },
	{ "findSubstrings", json(findSubstrings) },
	{ "normalise", jarr(fnNormalise) },
	{ "spec", jarr(fnSpec) },
	{ "catalyst", jarr(fnCatalyst) },
	{ "tagSets", json(tagSets) },
	{ "sort", jarr(fnSort) },
	{ "stringify", jarr(strOut) },
	{ "save", saveOut },
	{ "saveErr", saveErr },
	{ "delve", json(delve) },
})
