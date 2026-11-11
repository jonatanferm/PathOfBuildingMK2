-- Gallery "u28_timeless_list" (U28): the real TimelessJewelListControl over a stand-in `build`
-- (timelessData with searchResults/sharedResults, outputRevision, spec.allocNodes and an itemsTab
-- whose methods record what they are called with). Rows with and without their own socket, an
-- unallocated socket, socket -1, desired-node lists with and without names, a zero weight;
-- hover tooltips for all eleven jewel types (GetJewelItem, Militant Faith with random devotion
-- variants through math.random), shift-select highlighting (OverrideSelectIndex +
-- SetHighlightColor), double click (OnSelClick: AddItem, SetSelItemId, PopulateSlots,
-- AddUndoState, the greyed label), the "added" tooltip, ScrollToIndex, and the cleared tooltip
-- while a button holds the focus (noTooltip). `new("Item"):Item(raw)` is a stand-in class that
-- keeps the raw text (Classes/Item.lua needs the game data); the stand-in AddItemStatDifferences
-- prints it. Rust counterpart: src/gallery/u28_timeless_list.rs. Interactions:
-- u28_timeless_list.pobscript.

local t_insert, t_remove, t_concat = table.insert, table.remove, table.concat

local ItemClass = newClass("Item")
function ItemClass:Item(raw)
	self.raw = raw
	return self
end

local typeLabels = { "Glorious Vanity", "Lethal Pride", "Brutal Restraint", "Militant Faith", "Elegant Hubris",
	"Heroic Tragedy", "Festering Vengeance", "Extinguishing Grasp", "Baleful Dominion", "Destructive Aspiration",
	"Reclaimed Malevolence" }

local G = { }

function G:Init()
	local events = { }
	local function event(s)
		t_insert(events, s)
		if #events > 4 then
			t_remove(events, 1)
		end
	end
	local sockets = { }
	for _, id in ipairs({ 26725, 54127, 7960 }) do
		sockets[id] = {
			slotName = "Jewel " .. id,
			SetSelItemId = function(slot, selItemId)
				event("sel " .. slot.slotName .. " " .. tostring(selItemId))
			end,
		}
	end
	local shared = {
		type = { id = 1, label = typeLabels[1] },
		conqueror = { id = 2 },
		devotionVariant1 = { id = 1 },
		devotionVariant2 = { id = 1 },
		socket = { id = 26725, keystone = "Acrobatics", label = "Acrobatics: 26725" },
		desiredNodes = {
			vaal_small_a = { desiredIdx = 1, displayName = "Small A" },
			vaal_notable_b = { desiredIdx = 2, displayName = "Notable B" },
		},
	}
	local rows = { }
	for i = 1, 24 do
		local row = { label = "^7Seed " .. (1000 + i * 7) .. "  weight " .. (i * 1.5), seed = 1000 + i * 7, total = (i == 5) and 0 or i * 1.5 }
		row.vaal_small_a = { targetNodeNames = { "Name " .. i, "Other " .. i } }
		if i % 2 == 0 then
			row.vaal_notable_b = { targetNodeNames = { } }
		end
		if i == 3 then
			row.socketId = 54127
			row.socketLabel = "Unallocated: 54127"
		elseif i == 4 then
			row.socketId = -1
		elseif i == 6 then
			row.socketId = 7960
			row.socketLabel = "Invalid slot: 7960"
		end
		t_insert(rows, row)
	end
	local nextId = 0
	self.build = {
		timelessData = { socketAllocate = true, searchResults = rows, sharedResults = shared },
		outputRevision = 1,
		spec = { allocNodes = { [26725] = true, [7960] = true } },
		itemsTab = {
			sockets = sockets,
			GetSocketAndJewelForNodeID = function(itemsTab, nodeId)
				return itemsTab.sockets[nodeId], nil
			end,
			AddItemStatDifferences = function(itemsTab, tooltip, item, base, slot)
				tooltip:AddLine(14, "^8slot " .. tostring(slot and slot.slotName))
				for line in item.raw:gmatch("[^\n]+") do
					tooltip:AddLine(12, "^7" .. line)
				end
			end,
			AddItem = function(itemsTab, item, noAutoEquip)
				nextId = nextId + 1
				item.id = nextId
				event("add " .. item.id .. " " .. tostring(noAutoEquip))
			end,
			IsItemValidForSlot = function(itemsTab, item, slotName)
				return slotName ~= "Jewel 7960"
			end,
			PopulateSlots = function(itemsTab)
				event("populate")
			end,
			AddUndoState = function(itemsTab)
				event("undo")
			end,
		},
	}
	local build = self.build
	self.host = new("ControlHost"):ControlHost()
	local c = self.host.controls
	c.list = new("TimelessJewelListControl"):TimelessJewelListControl(nil, {20, 40, 360, 300}, build)
	c.type = new("ButtonControl"):ButtonControl(nil, {420, 40, 160, 20}, "Next jewel type", function()
		shared.type.id = shared.type.id % 11 + 1
		shared.type.label = typeLabels[shared.type.id]
		shared.conqueror.id = shared.type.id % 3 + 1
		build.outputRevision = build.outputRevision + 1
	end)
	c.devotion = new("ButtonControl"):ButtonControl(nil, {420, 70, 160, 20}, "Devotion 1 = 5", function()
		shared.devotionVariant1.id = 5
		build.outputRevision = build.outputRevision + 1
	end)
	c.scroll = new("ButtonControl"):ButtonControl(nil, {420, 100, 160, 20}, "Scroll to 15", function()
		c.list:ScrollToIndex(15)
	end)
	c.status = new("LabelControl"):LabelControl(nil, {20, 10, 0, 16}, function()
		return "^7sel " .. tostring(c.list.selIndex) .. " hl " .. tostring(c.list.highlightIndex) .. " type " .. shared.type.id .. " flag " .. tostring(build.buildFlag) .. " | " .. t_concat(events, ", ")
	end)
end

function G:OnFrame(inputEvents, viewPort)
	self.host:ProcessControlsInput(inputEvents, viewPort)
	SetDrawColor(0.1, 0.1, 0.1)
	DrawImage(nil, viewPort.x, viewPort.y, viewPort.width, viewPort.height)
	self.host:DrawControls(viewPort)
end

return G
