-- INT-1-FRESH: does PoB 2.67.2 know everything a loaded build uses (gems, items, tree nodes)?
-- Run after each load with pob_headless (one PoB process, under the oracle lock):
--   HEADLESS_EXEC="$(cat scripts/port/fresh_corpus_datacheck.lua)" POB_ENGINE=jit \
--   POB_ROOT=.PathOfBuilding flock /tmp/pob-port-oracle.lock env POB_ORACLE_LOCK_HELD=1 \
--   target/release/pob_headless .parity/fresh/*.xml | grep -E '^DATA|^  - '
-- The unique-title lookup is a heuristic: generated uniques (Forbidden Flame/Flesh, ...) and
-- Foulborn uniques do not match it although PoB handles them (docs/port/parity/fresh-corpus.md).
local uniqueTitles
local function getUniqueTitles()
	if uniqueTitles then return uniqueTitles end
	uniqueTitles = {}
	for _, list in pairs(data.uniques) do
		for _, raw in ipairs(list) do
			local title = raw:match("^%s*([^\n]-)%s*\n")
			if title then uniqueTitles[title] = true end
		end
	end
	return uniqueTitles
end
local b = __mainObject__.main.modes["BUILD"]
local issues = {}
for _, group in ipairs(b.skillsTab.socketGroupList) do
	for _, gem in ipairs(group.gemList) do
		if gem.errMsg or (not gem.gemData and not gem.grantedEffect and (gem.nameSpec or ""):match("%S")) then
			issues[#issues + 1] = "gem " .. tostring(gem.nameSpec) .. " (" .. tostring(gem.errMsg) .. ")"
		end
	end
end
local nItems = 0
for _, item in pairs(b.itemsTab.items) do
	nItems = nItems + 1
	if not item.base then issues[#issues + 1] = "item without base: " .. tostring(item.name) end
	if (item.rarity == "UNIQUE" or item.rarity == "RELIC") and item.title and not getUniqueTitles()[item.title] then
		issues[#issues + 1] = "unique title not in data.uniques: " .. tostring(item.name)
	end
end
local xml = __pob_xml
local xmlItems = 0
for _ in xml:gmatch("<Item ") do xmlItems = xmlItems + 1 end
if xmlItems ~= nItems then issues[#issues + 1] = string.format("items: %d in XML, %d loaded", xmlItems, nItems) end
local spec = b.spec
local alloc = 0
for _ in pairs(spec.allocNodes) do alloc = alloc + 1 end
local nodesAttr = xml:match('<Spec [^>]-nodes="([^"]*)"') or ""
local xmlNodes = 0
local unknown = {}
for id in nodesAttr:gmatch("%d+") do
	xmlNodes = xmlNodes + 1
	if not spec.nodes[tonumber(id)] then unknown[#unknown + 1] = id end
end
if #unknown > 0 then issues[#issues + 1] = "tree nodes unknown to PoB: " .. table.concat(unknown, ",") end
return string.format("DATA %s: tree %s, %d/%d nodes allocated, %d items, %d issues%s", __pob_name,
	tostring(spec.treeVersion), alloc, xmlNodes, nItems, #issues,
	#issues > 0 and ("\n  - " .. table.concat(issues, "\n  - ")) or "")
