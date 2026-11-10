-- BM-A oracle dumps (crates/pob-oracle/tests/BM-A.rs).
--
-- Runs after BOOT_SCRIPT and oracle.lua, before the build loads. `__bma.dump()` returns the
-- loaded build model (Build section, items tab, tree tab, calcs tab, the skills/config set
-- skeleton) as sorted `path<TAB>value` lines; values are `b:`/`n:` (%.17g)/`s:` (control
-- bytes and `\` as `\ddd`). The Rust side (BM-A.rs `rust_dump`) writes the same lines.

local fmt = string.format
local B = { }
__bma = B
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
local function sortedKeys(t)
	local keys = { }
	for k in pairs(t) do keys[#keys + 1] = k end
	table.sort(keys, function(a, b)
		if type(a) == type(b) then return a < b end
		return type(a) == "number"
	end)
	return keys
end
local function numList(t)
	local l = { }
	for k in pairs(t) do l[#l + 1] = k end
	table.sort(l)
	for i, v in ipairs(l) do l[i] = fmt("%.17g", v) end
	return table.concat(l, ",")
end

function B.dump(build)
	out = { }
	build = build or __mainObject__.main.modes["BUILD"]
	-- Build section
	emit("build.targetVersion", build.targetVersion)
	emit("build.viewMode", build.viewMode)
	emit("build.characterLevel", build.characterLevel)
	emit("build.characterLevelAutoMode", build.characterLevelAutoMode)
	emit("build.bandit", build.bandit)
	emit("build.pantheonMajorGod", build.pantheonMajorGod)
	emit("build.pantheonMinorGod", build.pantheonMinorGod)
	emit("build.mainSocketGroup", build.mainSocketGroup)
	emit("build.spectreList", table.concat(build.spectreList, ","))
	local td = build.timelessData
	emit("build.timeless.jewelType", td.jewelType.id)
	emit("build.timeless.conquerorType", td.conquerorType.id)
	emit("build.timeless.devotionVariant1", td.devotionVariant1)
	emit("build.timeless.devotionVariant2", td.devotionVariant2)
	emit("build.timeless.jewelSocket", td.jewelSocket.id)
	emit("build.timeless.fallbackWeightMode", td.fallbackWeightMode.idx)
	emit("build.timeless.socketFilter", td.socketFilter)
	emit("build.timeless.socketAllocate", td.socketAllocate)
	emit("build.timeless.socketFilterDistance", td.socketFilterDistance)
	emit("build.timeless.searchList", td.searchList)
	emit("build.timeless.searchListFallback", td.searchListFallback)

	-- items tab
	local it = build.itemsTab
	emit("items.orderList", table.concat(it.itemOrderList, ","))
	for _, id in ipairs(it.itemOrderList) do
		local item = it.items[id]
		local p = "items.item." .. id
		emit(p .. ".name", item.name)
		emit(p .. ".baseName", item.baseName)
		emit(p .. ".rarity", item.rarity)
		emit(p .. ".type", item.type)
		emit(p .. ".raw", item.raw)
		for _, k in ipairs({ "variant", "variantAlt", "variantAlt2", "variantAlt3", "variantAlt4", "variantAlt5" }) do
			emit(p .. "." .. k, item[k])
		end
		for _, k in ipairs({ "hasAltVariant", "hasAltVariant2", "hasAltVariant3", "hasAltVariant4", "hasAltVariant5" }) do
			emit(p .. "." .. k, item[k])
		end
		local ranges = { }
		for _, list in ipairs({ item.buffModLines, item.enchantModLines, item.scourgeModLines, item.implicitModLines, item.explicitModLines, item.crucibleModLines }) do
			for _, l in ipairs(list) do
				ranges[#ranges + 1] = l.range and fmt("%.17g", l.range) or "-"
			end
		end
		emit(p .. ".ranges", table.concat(ranges, ","))
	end
	emit("items.activeItemSetId", it.activeItemSetId)
	emit("items.showStatDifferences", it.showStatDifferences)
	emit("items.itemSetOrderList", table.concat(it.itemSetOrderList, ","))
	for _, id in ipairs(sortedKeys(it.itemSets)) do
		local set = it.itemSets[id]
		local p = "items.set." .. id
		emit(p .. ".id", set.id)
		emit(p .. ".title", set.title)
		emit(p .. ".useSecondWeaponSet", set.useSecondWeaponSet)
		for _, k in ipairs(sortedKeys(set)) do
			local v = set[k]
			if type(v) == "table" then
				emit(p .. ".slot." .. k .. ".selItemId", v.selItemId)
				emit(p .. ".slot." .. k .. ".active", v.active)
				emit(p .. ".slot." .. k .. ".pbURL", v.pbURL)
			end
		end
	end
	emit("items.activeSet", it.activeItemSet == it.itemSets[it.activeItemSetId])
	local ordered = { }
	for i, slot in ipairs(it.orderedSlots) do ordered[i] = slot.slotName end
	emit("items.orderedSlots", table.concat(ordered, "|"))
	for _, name in ipairs(sortedKeys(it.slots)) do
		local slot = it.slots[name]
		local p = "items.slot." .. name
		emit(p .. ".selItemId", slot.selItemId)
		emit(p .. ".active", slot.active)
		emit(p .. ".inactive", slot.inactive)
		emit(p .. ".weaponSet", slot.weaponSet)
		emit(p .. ".nodeId", slot.nodeId)
		emit(p .. ".slotNum", slot.slotNum)
		emit(p .. ".label", slot.label)
		emit(p .. ".parentSlot", slot.parentSlot and slot.parentSlot.slotName)
		local ids = { }
		for _, id in ipairs(slot.items) do ids[#ids + 1] = id end
		table.sort(ids)
		emit(p .. ".items", table.concat(ids, ","))
		if slot.controls.activate then
			emit(p .. ".activateState", slot.controls.activate.state)
		end
	end
	emit("items.sockets", numList(it.sockets))
	emit("items.lastSlot", it.lastSlot and it.lastSlot.slotName)
	local ssl = it.tradeQuery.statSortSelectionList
	if ssl then
		emit("items.statSort.n", #ssl)
		for i, s in ipairs(ssl) do
			emit("items.statSort." .. i .. ".label", s.label)
			emit("items.statSort." .. i .. ".stat", s.stat)
			emit("items.statSort." .. i .. ".weightMult", s.weightMult)
		end
	end

	-- tree tab
	local tt = build.treeTab
	emit("tree.activeSpec", tt.activeSpec)
	emit("tree.n", #tt.specList)
	emit("tree.buildSpecIsActive", build.spec == tt.specList[tt.activeSpec])
	for i, spec in ipairs(tt.specList) do
		local p = "tree.spec." .. i
		emit(p .. ".treeVersion", spec.treeVersion)
		emit(p .. ".title", spec.title)
		emit(p .. ".curClassId", spec.curClassId)
		emit(p .. ".curAscendClassId", spec.curAscendClassId)
		emit(p .. ".curSecondaryAscendClassId", spec.curSecondaryAscendClassId)
		emit(p .. ".alloc", numList(spec.allocNodes))
		local j = { }
		for _, k in ipairs(sortedKeys(spec.jewels)) do j[#j + 1] = k .. "=" .. tostring(spec.jewels[k]) end
		emit(p .. ".jewels", table.concat(j, ","))
		local m = { }
		for _, k in ipairs(sortedKeys(spec.masterySelections)) do m[#m + 1] = k .. "=" .. tostring(spec.masterySelections[k]) end
		emit(p .. ".mastery", table.concat(m, ","))
	end

	-- calcs tab
	local ct = build.calcsTab
	for _, k in ipairs(sortedKeys(ct.input)) do emit("calcs.input." .. k, ct.input[k]) end
	for _, section in ipairs(ct.sectionList) do
		for _, sub in ipairs(section.subSection) do
			emit("calcs.section." .. section.id .. "." .. sub.id, sub.collapsed)
		end
	end

	-- skills / config skeleton (BM-B checks the contents)
	local st = build.skillsTab
	emit("skills.activeSkillSetId", st.activeSkillSetId)
	emit("skills.skillSetOrderList", table.concat(st.skillSetOrderList, ","))
	emit("skills.groups", #st.socketGroupList)
	emit("skills.defaultGemLevel", st.defaultGemLevel)
	emit("skills.defaultGemQuality", st.defaultGemQuality)
	local cfg = build.configTab
	emit("config.activeConfigSetId", cfg.activeConfigSetId)
	emit("config.configSetOrderList", table.concat(cfg.configSetOrderList, ","))
	for _, k in ipairs(sortedKeys(cfg.input)) do
		local v = cfg.input[k]
		if type(v) ~= "table" then emit("config.input." .. k, v) end
	end
	return table.concat(out, "\n")
end

-- Hook: the first BuildOutput of the build load (Build.lua:694). The Rust loader stops
-- before it (`loadBuildModelFromXML`), so this is the model to compare with. `B.preSave` is
-- PoB's `SaveDB` text at that point; `Build:Save` needs `calcsTab.mainEnv` for the display
-- stats, so the stat lists are emptied and a stand-in env is set for the call only.
do
	if not common.classes.CalcsTab then LoadModule("Classes/CalcsTab") end
	local CT = common.classes.CalcsTab
	local orig = CT.BuildOutput
	CT.BuildOutput = function(self, ...)
		if not B.pre then
			local build = self.build
			local ok, r = pcall(B.dump, build)
			B.pre = ok and r or ("error\t" .. tostring(r))
			local ds, ms, es, env = build.displayStats, build.minionDisplayStats, build.extraSaveStats, self.mainEnv
			build.displayStats, build.minionDisplayStats, build.extraSaveStats = { }, { }, { }
			self.mainEnv = { player = { mainSkill = { skillFlags = { } } } }
			local ok2, s = pcall(build.SaveDB, build, "bma")
			B.preSave = ok2 and s or ("error\t" .. tostring(s))
			build.displayStats, build.minionDisplayStats, build.extraSaveStats, self.mainEnv = ds, ms, es, env
		end
		return orig(self, ...)
	end
end
