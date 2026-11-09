-- IT-C oracle (crates/pob-oracle/tests/IT-C.rs): runs inside a booted PoB (LuaJIT, jit.off).
--
-- Parses every unique (all variants), every rare, every item of the build corpus and synthetic
-- items, as IT-A's oracle does, and records what IT-C's code computes:
--   * per ParseRaw call ("I" records): the outputs of Item:BuildModList (modList, slotModList[1..3],
--     baseModList, buffModList, weaponData, armourData, flaskData, tinctureData, jewelData and the
--     item fields BuildModList writes), every mod line's modList/showSlider after it, and
--     itemLib.formatModLine (both modes) / itemLib.isModLineScalable of every mod line; plus the
--     Item:CanHaveMod results ParseRaw asked for;
--   * every modLib.parseMod call (unique by arguments, "P" records): the Rust side replays them,
--     since the ModParser lists are not all ported yet (the parser is not IT-C's);
--   * every itemLib.applyRange call (unique by arguments, "A" records), including a sweep over
--     every ranged line of data.uniques with several ranges and scalars.
--
-- Inputs (globals set by the test's prologue):
--   __itc_out    = path of the JSONL output file
--   __itc_corpus = { { src, raw, id, variant, variantAlt..variantAlt5 }, ... }
--   __itc_synth  = { "<item text>", ... }   synthetic items (parsed as pasted: rarity nil)
--   __itc_limit  = max uniques (nil = all)
-- Returns a JSON header (counts).

new("Item"):Item() -- loads the class
local ItemClass = common.classes.Item

local outFile = assert(io.open(__itc_out, "w"))

-- ---- JSON ------------------------------------------------------------------------------------
local function encstr(s)
	return '"' .. s:gsub('[%c"\\\128-\255]', function(c)
		if c == '"' then return '\\"' elseif c == "\\" then return "\\\\" end
		return string.format("\\u%04x", c:byte())
	end) .. '"'
end
local function encnum(v)
	if v ~= v then return '"$nan"' elseif v == math.huge then return '"$inf"' elseif v == -math.huge then return '"$-inf"' end
	return string.format("%.17g", v)
end
local function enckey(k)
	local t = type(k)
	if t == "string" then return encstr(k) elseif t == "number" then return encnum(k) elseif t == "boolean" then return k and '{"$b":true}' or '{"$b":false}' end
	return '"$badkey"'
end
-- pob-dump shape (crates/pob-dump/FORMAT.md) for mods, so serde builds pob_gamedata::Mod
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
-- generic state encoder ($t = list of [key, value]; keys keep their Lua type)
local enc
enc = function(v, out, depth)
	local t = type(v)
	if t == "nil" then out[#out + 1] = "null"
	elseif t == "boolean" then out[#out + 1] = v and "true" or "false"
	elseif t == "number" then out[#out + 1] = encnum(v)
	elseif t == "string" then out[#out + 1] = encstr(v)
	elseif t == "table" then
		if v.name and v.type and v.flags and v.keywordFlags then
			out[#out + 1] = '{"$mods":' .. encodeMods({ v }) .. '}'
			return
		end
		if depth > 12 then out[#out + 1] = '{"$deep":true}' return end
		out[#out + 1] = '{"$t":['
		local first = true
		for k, x in pairs(v) do
			if not first then out[#out + 1] = "," end
			first = false
			out[#out + 1] = "[" .. enckey(k) .. ","
			enc(x, out, depth + 1)
			out[#out + 1] = "]"
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
local function rangeJson(r)
	if type(r) == "table" then
		local p = { }
		for i, x in ipairs(r) do p[i] = encnum(x) end
		return "[" .. table.concat(p, ",") .. "]"
	elseif type(r) == "number" then return encnum(r)
	elseif r == nil then return "null" end
	return encstr(tostring(r))
end
local function optnum(x)
	if type(x) == "number" then return encnum(x) end
	return "null"
end

-- ---- recording ---------------------------------------------------------------------------------
local seenParse = { }
local seenApply = { }
local nP, nA = 0, 0

local origParseMod = modLib.parseMod
modLib.parseMod = function(line, isComb)
	local modList, extra = origParseMod(line, isComb)
	local key = line .. "\1" .. (isComb and "1" or "0")
	if not seenParse[key] then
		seenParse[key] = true
		nP = nP + 1
		outFile:write('{"t":"P","line":', encstr(line), ',"comb":', isComb and "true" or "false",
			',"mods":', encodeMods(modList), ',"extra":', extra and encstr(extra) or "null", "}\n")
	end
	return modList, extra
end

local function rangeKey(r)
	if type(r) == "table" then
		local p = { }
		for i, x in ipairs(r) do p[i] = string.format("%.17g", x) end
		return "{" .. table.concat(p, ",") .. "}"
	end
	return type(r) == "number" and string.format("%.17g", r) or tostring(r)
end
local origApplyRange = itemLib.applyRange
itemLib.applyRange = function(line, range, valueScalar, baseValueScalar)
	local ok, r = pcall(origApplyRange, line, range, valueScalar, baseValueScalar)
	local key = tostring(line) .. "\1" .. rangeKey(range) .. "\1" .. tostring(valueScalar) .. "\1" .. tostring(baseValueScalar)
	if type(line) == "string" and not seenApply[key] then
		seenApply[key] = true
		nA = nA + 1
		outFile:write('{"t":"A","line":', encstr(line), ',"range":', rangeJson(range), ',"vs":', optnum(valueScalar),
			',"bvs":', optnum(baseValueScalar), ',"r":', ok and type(r) == "string" and encstr(r) or "null",
			',"err":', ok and "null" or encstr(tostring(r)), "}\n")
	end
	if not ok then error(r, 0) end
	return r
end

local curItem, curCan
local origCanHaveMod = ItemClass.CanHaveMod
ItemClass.CanHaveMod = function(self, mod, includeTags)
	local r = origCanHaveMod(self, mod, includeTags)
	if curCan and self == curItem then
		local id = "?"
		for k, v in pairs(self.affixes or { }) do
			if v == mod then id = k break end
		end
		curCan[#curCan + 1] = "[" .. encstr(id) .. "," .. (r and "true" or "false") .. "]"
	end
	return r
end

-- ---- item outputs --------------------------------------------------------------------------------
local lineLists = { "enchantModLines", "scourgeModLines", "classRequirementModLines", "implicitModLines",
	"explicitModLines", "crucibleModLines", "buffModLines" }

local function jewelDataJson(jd)
	if not jd then return "null" end
	local copy = { }
	for k, v in pairs(jd) do copy[k] = v end
	if jd.funcList then
		local fl = { }
		for i, f in ipairs(jd.funcList) do fl[i] = "$jf:" .. tostring(type(f) == "table" and f.type) end
		copy.funcList = fl
	end
	return json(copy)
end

local function dumpOut(item)
	local p = { }
	local function put(k, v) p[#p + 1] = encstr(k) .. ":" .. v end
	put("modList", encodeMods(item.modList))
	if item.slotModList then
		put("slotModList", "[" .. encodeMods(item.slotModList[1]) .. "," .. encodeMods(item.slotModList[2]) .. ","
			.. encodeMods(item.slotModList[3]) .. "]")
	else
		put("slotModList", "null")
	end
	put("baseModList", encodeMods(item.baseModList))
	put("buffModList", encodeMods(item.buffModList))
	put("weaponData", json(item.weaponData))
	put("armourData", json(item.armourData))
	put("flaskData", json(item.flaskData))
	put("tinctureData", json(item.tinctureData))
	put("jewelData", jewelDataJson(item.jewelData))
	local f = { }
	for _, k in ipairs({ "requirements", "grantedSkills", "sockets", "abyssalSocketCount", "selectableSocketCount",
			"socketedJewelEffectModifier", "classRestriction", "hasModTags", "craftedQuality", "quality",
			"corruptImplicitCount", "clusterJewelSkill", "clusterJewelNodeCount", "modSource" }) do
		f[#f + 1] = encstr(k) .. ":" .. json(item[k])
	end
	f[#f + 1] = '"colourAlwaysMatches":' .. json(item.sockets and item.sockets.colourAlwaysMatches)
	put("fields", "{" .. table.concat(f, ",") .. "}")
	local rl = { }
	for i, l in ipairs(item.rangeLineList or { }) do rl[i] = encstr(tostring(l.line)) end
	put("rangeLineList", item.rangeLineList and ("[" .. table.concat(rl, ",") .. "]") or "null")
	local ll = { }
	for _, listName in ipairs(lineLists) do
		local lines = { }
		for i, l in ipairs(item[listName] or { }) do
			local okF, fmt = pcall(itemLib.formatModLine, l, false)
			local okD, fmtDb = pcall(itemLib.formatModLine, l, true)
			local scal = "null"
			if l.range and type(l.line) == "string" then
				local okS, s = pcall(itemLib.isModLineScalable, l.line, l.range, l.valueScalar)
				scal = okS and (s and "true" or "false") or '"err"'
			end
			lines[i] = "{" .. '"showSlider":' .. (l.showSlider and "true" or "false")
				.. ',"modList":' .. (type(l.modList) == "table" and encodeMods(l.modList) or "null")
				.. ',"fmt":' .. (okF and (fmt and encstr(fmt) or "null") or '"$err"')
				.. ',"fmtDb":' .. (okD and (fmtDb and encstr(fmtDb) or "null") or '"$err"')
				.. ',"scal":' .. scal .. "}"
		end
		ll[#ll + 1] = encstr(listName) .. ":[" .. table.concat(lines, ",") .. "]"
	end
	put("lines", "{" .. table.concat(ll, ",") .. "}")
	return "{" .. table.concat(p, ",") .. "}"
end

-- ---- driver ----------------------------------------------------------------------------------------
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
	curItem, curCan = item, { }
	local ok, err = pcall(item.ParseRaw, item, raw, rarity, highQuality)
	local can = curCan
	curItem, curCan = nil, nil
	nParses = nParses + 1
	local out = "null"
	if ok then
		local okO, o = pcall(dumpOut, item)
		out = okO and o or ('{"$dumpErr":' .. encstr(tostring(o)) .. '}')
	end
	local s = "{" .. '"raw":' .. encstr(raw) .. ',"rarity":' .. (rarity and encstr(rarity) or "null")
		.. ',"highQuality":' .. (highQuality and "true" or "false") .. ',"setup":' .. setupJson(setup)
		.. ',"can":[' .. table.concat(can, ",") .. ']'
		.. ',"out":' .. out .. ',"err":' .. (ok and "null" or encstr(tostring(err))) .. "}"
	return s, ok
end

local function emit(src, kind, parses)
	outFile:write('{"t":"I","src":', encstr(src), ',"kind":', encstr(kind), ',"parses":[', table.concat(parses, ","), "]}\n")
	nItems = nItems + 1
end

local counts = { }
local function count(kind) counts[kind] = (counts[kind] or 0) + 1 end

local types = { }
for t in pairs(data.uniques) do types[#types + 1] = t end
table.sort(types)
local nUniques = 0
for _, t in ipairs(types) do
	for i, raw in ipairs(data.uniques[t]) do
		if __itc_limit and nUniques >= __itc_limit then break end
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

for i, raw in ipairs(data.rares) do
	local item = new("Item"):Item()
	local s = doParse(item, sanitiseText(raw), "RARE", true)
	emit("rare#" .. i, "rare", { s })
	count("rare")
end

for _, entry in ipairs(__itc_corpus or { }) do
	local item = new("Item"):Item()
	local s1 = doParse(item, sanitiseText(""), nil, false)
	local s2 = doParse(item, entry.raw, nil, false, entry)
	emit(entry.src, "corpus", { s1, s2 })
	count("corpus")
end

for i, raw in ipairs(__itc_synth or { }) do
	local item = new("Item"):Item()
	local s = doParse(item, sanitiseText(raw), nil, false)
	emit("synthetic#" .. i, "synthetic", { s })
	count("synthetic")
end

-- applyRange sweep: every ranged line of every unique, with several ranges and scalars
local sweep = 0
local sweepSeen = { }
for _, t in ipairs(types) do
	for _, raw in ipairs(data.uniques[t]) do
		for line in raw:gmatch("[^\n]+") do
			local l = line
			while l:match("^{[^}]*}") do l = l:gsub("^{[^}]*}", "", 1) end
			if l:match("%(%-?%d+%.?%d*%-%-?%d+%.?%d*%)") and not sweepSeen[l] then
				sweepSeen[l] = true
				for _, c in ipairs({ { 0.37 }, { 1, 1.5 }, { 0, nil, 1.22 }, { 0.5, 1.3, 1.22 }, { { 0.2, 0.9 }, 1.3 },
						{ { 0.25 } }, { 0.84, 2, 1 } }) do
					pcall(itemLib.applyRange, l, c[1], c[2], c[3])
					sweep = sweep + 1
				end
			end
		end
	end
end

outFile:close()

local cparts = { }
for k, n in pairs(counts) do cparts[#cparts + 1] = encstr(k) .. ":" .. n end
return '{"items":' .. nItems .. ',"parses":' .. nParses .. ',"parseMod":' .. nP .. ',"applyRange":' .. nA
	.. ',"sweep":' .. sweep .. ',"counts":{' .. table.concat(cparts, ",") .. '}'
	.. ',"defaultItemAffixQuality":' .. (main and main.defaultItemAffixQuality and encnum(main.defaultItemAffixQuality) or "null")
	.. ',"notSupportedModTooltips":' .. ((main and main.notSupportedModTooltips) and "true" or "false")
	.. ',"notSupportedTooltipText":' .. ((main and main.notSupportedTooltipText) and encstr(main.notSupportedTooltipText) or "null")
	.. ',"devModeAlt":' .. ((launch and launch.devModeAlt) and "true" or "false")
	.. '}'
