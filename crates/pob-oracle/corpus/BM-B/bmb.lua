-- BM-B oracle dumps (crates/pob-oracle/tests/BM-B.rs).
--
-- Runs after BOOT_SCRIPT and oracle.lua (for __oracle_json), before the build
-- loads. Captures the skills/config/party model the moment the build has
-- loaded and `skillsTab:UpdateSocketGroups()` ran (Build.lua:691), i.e. at the
-- first `calcsTab:BuildOutput()` (Build.lua:694), before any calc mutates the
-- socket groups. Every dump is a list of `path<TAB>value` lines; values are
-- `b:`/`n:` (%.17g)/`s:` (control bytes and `\` as `\ddd`) or `j:` + JSON.

local fmt = string.format
local B = { }
__bmb = B
local out

local function enc(v)
	local t = type(v)
	if t == "nil" then return nil end
	if t == "boolean" then return "b:" .. tostring(v) end
	if t == "number" then return "n:" .. fmt("%.17g", v) end
	if t == "string" then
		return "s:" .. (v:gsub("[%c\\]", function(c) return fmt("\\%03d", c:byte()) end))
	end
	return "t:" .. t
end
local function emit(path, v)
	local e = enc(v)
	if e then out[#out + 1] = path .. "\t" .. e end
end
local function emitJson(path, v)
	out[#out + 1] = path .. "\tj:" .. __oracle_json(v, 64)
end

-- An xml node as JSON: {"elem":…,"attrib":{…},"children":[…]} (oracle.lua's generic encoder
-- would turn a <Gem> attrib table into a `$gemInstance` marker). Non-string attributes are
-- dropped, as composeNode does.
local function jstr(s)
	return '"' .. s:gsub('[%c"\\]', function(c)
		if c == '"' then return '\\"' elseif c == "\\" then return "\\\\" end
		return fmt("\\u%04x", c:byte())
	end) .. '"'
end
local function xmlJson(node)
	if type(node) == "string" then return jstr(node) end
	local parts = { '{"elem":', jstr(tostring(node.elem)), ',"attrib":{' }
	local keys = { }
	for k, v in pairs(node.attrib or { }) do
		if type(v) == "string" then keys[#keys + 1] = k end
	end
	table.sort(keys)
	for i, k in ipairs(keys) do
		parts[#parts + 1] = (i > 1 and "," or "") .. jstr(k) .. ":" .. jstr(node.attrib[k])
	end
	parts[#parts + 1] = '},"children":['
	for i, c in ipairs(node) do
		parts[#parts + 1] = (i > 1 and "," or "") .. xmlJson(c)
	end
	parts[#parts + 1] = "]}"
	return table.concat(parts)
end
local function emitXml(path, node)
	out[#out + 1] = path .. "\tj:" .. xmlJson(node)
end

local GROUPF = { "label", "enabled", "includeInFullDPS", "groupCount", "slot", "source", "mainActiveSkill",
	"mainActiveSkillCalcs", "imbuedSupport" }
local GEMF = { "nameSpec", "skillId", "gemId", "level", "quality", "enabled", "enableGlobal1", "enableGlobal2", "count",
	"skillPart", "skillPartCalcs", "skillStageCount", "skillStageCountCalcs", "skillMineCount", "skillMineCountCalcs",
	"skillMinion", "skillMinionCalcs", "skillMinionItemSet", "skillMinionItemSetCalcs", "skillMinionSkill",
	"skillMinionSkillCalcs", "color", "errMsg", "reqLevel", "reqStr", "reqDex", "reqInt", "matchesSocket",
	"naturalMaxLevel", "new", "triggered" }

local function dumpGroups(prefix, list)
	emit(prefix .. ".n", #list)
	for i, g in ipairs(list) do
		local p = prefix .. "[" .. i .. "]"
		for _, f in ipairs(GROUPF) do emit(p .. "." .. f, g[f]) end
		emit(p .. ".gems.n", #g.gemList)
		for j, gem in ipairs(g.gemList) do
			local q = p .. ".gems[" .. j .. "]"
			for _, f in ipairs(GEMF) do emit(q .. "." .. f, gem[f]) end
			emit(q .. ".gemData", gem.gemData and gem.gemData.id)
			emit(q .. ".grantedEffect", gem.grantedEffect and gem.grantedEffect.id)
		end
	end
end

function B.dumpSkills(st)
	emit("skills.activeSkillSetId", st.activeSkillSetId)
	emit("skills.defaultGemLevel", st.defaultGemLevel)
	emit("skills.defaultGemQuality", st.defaultGemQuality)
	emit("skills.sortGemsByDPS", st.sortGemsByDPS)
	emit("skills.sortGemsByDPSField", st.sortGemsByDPSField)
	emit("skills.showSupportGemTypes", st.showSupportGemTypes)
	emit("skills.showLegacyGems", st.showLegacyGems)
	for i, id in ipairs(st.skillSetOrderList) do
		emit("skills.order[" .. i .. "]", id)
		local set = st.skillSets[id]
		emit("skills.sets[" .. id .. "].title", set.title)
		dumpGroups("skills.sets[" .. id .. "].groups", set.socketGroupList)
	end
	local active = st.skillSets[st.activeSkillSetId]
	emit("skills.activeIsSet", active ~= nil and rawequal(st.socketGroupList, active.socketGroupList))
	dumpGroups("skills.groups", st.socketGroupList)
	for i, g in ipairs(st.socketGroupList) do
		if g == st.displayGroup then emit("skills.displayGroup", i) end
	end
	for slot, ge in pairs(st.imbuedSupportBySlot) do emit("skills.imbued." .. slot, ge.id) end
end

function B.dumpConfig(ct, prefix)
	prefix = prefix or "cfg"
	emit(prefix .. ".activeConfigSetId", ct.activeConfigSetId)
	for i, id in ipairs(ct.configSetOrderList) do emit(prefix .. ".order[" .. i .. "]", id) end
	for id, set in pairs(ct.configSets) do
		local p = prefix .. ".sets[" .. tostring(id) .. "]"
		emit(p .. ".id", set.id)
		emit(p .. ".title", set.title)
		for k, v in pairs(set.input) do emit(p .. ".input." .. k, v) end
		for k, v in pairs(set.placeholder) do emit(p .. ".placeholder." .. k, v) end
		if set.customModsList then
			emit(p .. ".cml.n", #set.customModsList)
			for i, b in ipairs(set.customModsList) do
				emit(p .. ".cml[" .. i .. "].title", b.title)
				emit(p .. ".cml[" .. i .. "].enabled", b.enabled)
				emit(p .. ".cml[" .. i .. "].text", b.text)
			end
		end
	end
	local active = ct.configSets[ct.activeConfigSetId]
	emit(prefix .. ".inputIsActive", active ~= nil and rawequal(ct.input, active.input))
	emit(prefix .. ".placeholderIsActive", active ~= nil and rawequal(ct.placeholder, active.placeholder))
end

local function dumpEntry(p, e)
	emit(p .. ".effectMult", e.effectMult)
	emit(p .. ".isMark", e.isMark)
	local i = 0
	for _, m in ipairs(e.modList) do
		i = i + 1
		emit(p .. ".mod[" .. i .. "]", modLib.formatSourceMod(m))
	end
	emit(p .. ".mods.n", i)
end

function B.dumpBuffList(prefix, list)
	for ty, entries in pairs(list) do
		if ty == "limit" then
			emit(prefix .. ".limit", entries)
		elseif type(entries) == "table" then
			for name, e in pairs(entries) do
				if name == "Vaal" and e.modList == nil then
					for vn, ve in pairs(e) do dumpEntry(prefix .. "." .. ty .. ".Vaal." .. vn, ve) end
				else
					dumpEntry(prefix .. "." .. ty .. "." .. name, e)
				end
			end
		end
	end
end

function B.dumpStore(prefix, store)
	if store.mods then
		local names = { }
		for name in pairs(store.mods) do names[#names + 1] = name end
		table.sort(names)
		for _, name in ipairs(names) do
			for i, m in ipairs(store.mods[name]) do
				emit(prefix .. "." .. name .. "[" .. i .. "]", modLib.formatSourceMod(m))
			end
		end
	else
		for i, m in ipairs(store) do emit(prefix .. "[" .. i .. "]", modLib.formatSourceMod(m)) end
	end
end

function B.dumpOutput(prefix, output)
	for k, v in pairs(output) do
		if type(v) == "table" then
			for k2, v2 in pairs(v) do emit(prefix .. "." .. k .. "." .. k2, v2) end
		else
			emit(prefix .. "." .. k, v)
		end
	end
end

local LABELS = { "simpleAuras", "simpleCurses", "simpleWarcries", "simpleLinks", "simpleEnemyMods", "simpleEnemyCond" }

function B.dumpParty(pt)
	for _, k in ipairs({ "Aura", "Curse", "Warcry", "Link" }) do B.dumpBuffList("party." .. k, pt.actor[k]) end
	B.dumpStore("party.modDB", pt.actor.modDB)
	B.dumpOutput("party.output", pt.actor.output)
	B.dumpStore("party.enemyModList", pt.enemyModList)
	for _, c in ipairs(LABELS) do emit("party.label." .. c, pt.controls[c].label) end
	for _, c in ipairs({ "editPartyMemberStats", "editAuras", "editCurses", "editWarcries", "editLinks", "enemyCond",
		"enemyMods" }) do
		emit("party.buf." .. c, pt.controls[c].buf)
	end
end

-- What the Rust side injects instead of loading items (BM-A/IT-*): the slots'
-- selected items and those items' sockets.
function B.dumpInjection(build)
	emit("inj.characterLevel", build.characterLevel)
	for slotName, slot in pairs(build.itemsTab.slots) do
		if slot.selItemId and slot.selItemId ~= 0 then emit("inj.slot." .. slotName, slot.selItemId) end
	end
	for id, item in pairs(build.itemsTab.items) do
		if item.sockets then
			emit("inj.item." .. id .. ".n", #item.sockets)
			emit("inj.item." .. id .. ".cam", item.sockets.colourAlwaysMatches)
			for k, s in ipairs(item.sockets) do emit("inj.item." .. id .. ".sock[" .. k .. "]", s.color) end
		end
	end
end

local function sectionNodes(xmlText)
	local root = common.xml.ParseXML(xmlText)
	local r = { }
	for _, node in ipairs(root[1]) do
		if type(node) == "table" and (node.elem == "Skills" or node.elem == "Config" or node.elem == "Party") then
			r[node.elem] = r[node.elem] or node
		end
	end
	return r
end

local function capture(build)
	out = { }
	B.dumpSkills(build.skillsTab)
	B.dumpConfig(build.configTab)
	B.dumpParty(build.partyTab)
	emit("cfg.enemyLevel", build.configTab.enemyLevel)
	for var, v in pairs(build.configTab.defaultState) do emit("cfg.defaultState." .. var, v) end
	B.dumpInjection(build)
	B.appendPre()
	for elem, node in pairs(sectionNodes(__pob_xml)) do emitXml("xml." .. elem, node) end
	local n = { elem = "Skills" }
	build.skillsTab:Save(n)
	emitXml("save.Skills", n)
	n = { elem = "Config" }
	build.configTab:Save(n)
	emitXml("save.Config", n)
	n = { elem = "Party" }
	build.partyTab:Save(n)
	emitXml("save.Party", n)
	return table.concat(out, "\n")
end

-- Hook: the first BuildOutput of the build load (Build.lua:694).
do
	if not common.classes.CalcsTab then LoadModule("Classes/CalcsTab") end
	local CT = common.classes.CalcsTab
	local orig = CT.BuildOutput
	CT.BuildOutput = function(self, ...)
		if not B.captured then
			local ok, r = pcall(capture, self.build)
			B.captured = ok and r or ("error\t" .. tostring(r))
		end
		return orig(self, ...)
	end
end

-- Hook: the config model as ConfigTab:Load left it, before the BuildModList that
-- SetActiveConfigSet runs (ConfigOptions apply functions write placeholders; CO-C).
-- `cfgpre.*` lines and `save.ConfigPre`; enemyLevel is UpdateLevel's (BuildModList's
-- first statement).
do
	if not common.classes.ConfigTab then LoadModule("Classes/ConfigTab") end
	local CfgT = common.classes.ConfigTab
	local origLoad, origBML = CfgT.Load, CfgT.BuildModList
	local inLoad, snapped = false, false
	CfgT.Load = function(self, ...)
		inLoad, snapped = true, false
		local r = origLoad(self, ...)
		inLoad = false
		return r
	end
	CfgT.BuildModList = function(self, ...)
		if inLoad and not snapped then
			snapped = true
			self:UpdateLevel()
			local saved = out
			out = { }
			B.dumpConfig(self, "cfgpre")
			emit("cfgpre.enemyLevel", self.enemyLevel)
			local n = { elem = "Config" }
			self:Save(n)
			emitXml("save.ConfigPre", n)
			B.cfgpre = out
			out = saved
		end
		return origBML(self, ...)
	end
end

function B.appendPre()
	for _, l in ipairs(B.cfgpre or { }) do out[#out + 1] = l end
end

local function isModT(t)
	return type(t) == "table" and type(rawget(t, "name")) == "string" and type(rawget(t, "type")) == "string"
end
local function conv(v)
	if type(v) ~= "table" or isModT(v) then return v end
	local r = { }
	for k, x in pairs(v) do
		if k == "modList" then
			local l = { }
			for _, m in ipairs(x) do l[#l + 1] = m end
			r.modList = l
		else
			r[k] = conv(x)
		end
	end
	return r
end

local EXPORT_TYPES = { "PlayerMods", "Aura", "Curse", "Warcry", "Link", "EnemyConditions", "EnemyMods" }

-- After the load: export the party buffs (CalcPerform.lua:3812-3931 → setBuffExports),
-- then exportBuffs per type, then ParseBuffs of those strings into fresh lists.
function B.exportStage()
	local build = __mainObject__.main.modes["BUILD"]
	local pt = build.partyTab
	out = { }
	pt.enableExportBuffs = true
	build.calcsTab:BuildOutput()
	emitJson("exports", conv(pt.buffExports or { }))
	local strs = { }
	for _, ty in ipairs(EXPORT_TYPES) do
		local ok, s = pcall(pt.exportBuffs, pt, ty)
		strs[ty] = ok and s or ""
		emit("str." .. ty, ok and s or ("ERROR " .. tostring(s)))
	end
	local actor = { Aura = { }, Curse = { }, Warcry = { }, Link = { }, modDB = new("ModDB"):ModDB(), output = { } }
	actor.modDB.actor = actor
	local enemyModList = new("ModList"):ModList()
	local labels = { }
	for _, c in ipairs(LABELS) do labels[c] = { label = "" } end
	local function run(ty, f)
		local ok, err = pcall(f)
		if not ok then emit("perr." .. ty, tostring(err)) end
	end
	run("PartyMemberStats", function() pt:ParseBuffs(actor.modDB, strs.PlayerMods, "PartyMemberStats", actor.output) end)
	run("Aura", function() pt:ParseBuffs(actor.Aura, strs.Aura, "Aura", labels.simpleAuras) end)
	run("Curse", function() pt:ParseBuffs(actor.Curse, strs.Curse, "Curse", labels.simpleCurses) end)
	run("Warcry", function() pt:ParseBuffs(actor.Warcry, strs.Warcry, "Warcry", labels.simpleWarcries) end)
	run("Link", function() pt:ParseBuffs(actor.Link, strs.Link, "Link", labels.simpleLinks) end)
	run("EnemyConditions", function() pt:ParseBuffs(enemyModList, strs.EnemyConditions, "EnemyConditions") end)
	run("EnemyMods", function() pt:ParseBuffs(enemyModList, strs.EnemyMods, "EnemyMods", labels.simpleEnemyMods) end)
	for _, k in ipairs({ "Aura", "Curse", "Warcry", "Link" }) do B.dumpBuffList("party." .. k, actor[k]) end
	B.dumpStore("party.modDB", actor.modDB)
	B.dumpOutput("party.output", actor.output)
	B.dumpStore("party.enemyModList", enemyModList)
	for _, c in ipairs(LABELS) do
		if c ~= "simpleEnemyCond" then emit("party.label." .. c, labels[c].label) end
	end
	return table.concat(out, "\n")
end

-- SkillsTab:ProcessGemLevel for every gem, every default gem level mode and a few character levels.
function B.processGemLevelStage()
	local build = __mainObject__.main.modes["BUILD"]
	local st = build.skillsTab
	out = { }
	local savedMode, savedLevel = st.defaultGemLevel, build.characterLevel
	for _, mode in ipairs({ "normalMaximum", "corruptedMaximum", "awakenedMaximum", "characterLevel", "levelOne", "bogus" }) do
		st.defaultGemLevel = mode
		for _, cl in ipairs({ 1, 12, 38, 70, 100 }) do
			build.characterLevel = cl
			for gemId, gemData in pairs(data.gems) do
				for _, imbued in ipairs({ false, true }) do
					local ok, r = pcall(st.ProcessGemLevel, st, gemData, imbued)
					emit("pgl." .. mode .. "." .. cl .. "." .. tostring(imbued) .. "." .. gemId, ok and r or "ERROR")
				end
			end
		end
	end
	st.defaultGemLevel, build.characterLevel = savedMode, savedLevel
	return table.concat(out, "\n")
end

-- Synthetic sections (crates/pob-oracle/corpus/BM-B/*.xml): Load them into the live build's tabs.
function B.syntheticStage(xmlText)
	B.cfgpre = nil
	local build = __mainObject__.main.modes["BUILD"]
	out = { }
	local nodes = sectionNodes(xmlText)
	if nodes.Skills then
		build.skillsTab:Load(nodes.Skills, "synthetic")
		build.skillsTab:UpdateSocketGroups()
		B.dumpSkills(build.skillsTab)
		local n = { elem = "Skills" }
		build.skillsTab:Save(n)
		emitXml("save.Skills", n)
	end
	if nodes.Config then
		build.configTab:Load(nodes.Config, "synthetic")
		B.dumpConfig(build.configTab)
		emit("cfg.enemyLevel", build.configTab.enemyLevel)
		local n = { elem = "Config" }
		build.configTab:Save(n)
		emitXml("save.Config", n)
	end
	B.dumpInjection(build)
	B.appendPre()
	for elem, node in pairs(nodes) do emitXml("xml." .. elem, node) end
	return table.concat(out, "\n")
end
