-- Gallery "u12_drag" (U12): ListControl drag and drop. Reorder inside a mutable list (the
-- insertion marker, OnOrderChange with both indices), cross-list drags to a list subclass with
-- CanReceiveDrag/ReceiveDrag (insertion at the target's selDragIndex, OnDragSend), a target whose
-- CanReceiveDrag refuses (never armed), targets WITHOUT CanReceiveDrag (presence semantics: a
-- PathControl with an instance ReceiveDrag, whose folder buttons turn green, and a plain list),
-- a BuildList-style list that is its own drag target with CanDragToValue (green rows, hover only
-- on valid rows, ReceiveDrag reading hoverValue), the drag text at layer 20 while the cursor is
-- off the source, and the ControlHost focus lock during a drag.
-- Rust counterpart: src/gallery/u12_drag.rs. Interactions: u12_drag.pobscript.

local t_insert = table.insert

local DragList = newClass("U12DragList", "ListControl")

function DragList:U12DragList(anchor, rect, list)
	self:ListControl(anchor, rect, 16, "VERTICAL", true, list)
	self.dragTargetList = { }
	return self
end

function DragList:GetRowValue(column, index, value)
	return "^7" .. value.label
end

function DragList:GetDragValue(index, value)
	return "Entry", value
end

function DragList:CanReceiveDrag(type, value)
	return type == "Entry"
end

function DragList:ReceiveDrag(type, value, source)
	t_insert(self.list, self.selDragIndex or #self.list + 1, { label = value.label .. "+" })
	self.lastEvent = "got " .. value.label
end

function DragList:OnOrderChange(selIndex, selDragIndex)
	self.lastEvent = "order " .. selIndex .. ">" .. selDragIndex
end

function DragList:OnDragSend(index, value, target)
	self.lastEvent = "sent " .. index .. " " .. value.label
end

local function rows(prefix, n, folderEvery)
	local list = { }
	for i = 1, n do
		t_insert(list, { label = prefix .. " " .. i, folder = folderEvery and i % folderEvery == 0 })
	end
	return list
end

local G = { }

function G:Init()
	self.host = new("ControlHost"):ControlHost()
	local c = self.host.controls
	c.src = new("U12DragList"):U12DragList(nil, {20, 40, 240, 200}, rows("Src", 8))
	c.dst = new("U12DragList"):U12DragList(nil, {300, 40, 240, 200}, rows("Dst", 3))
	c.reject = new("ListControl"):ListControl(nil, {580, 40, 200, 100}, 16, "VERTICAL", false, rows("Reject", 3))
	c.reject.GetRowValue = function(control, column, index, value)
		return value.label
	end
	function c.reject:CanReceiveDrag()
		return false
	end
	c.folders = new("ListControl"):ListControl(nil, {580, 180, 200, 120}, 16, "VERTICAL", false, rows("Node", 6, 2))
	c.folders.GetRowValue = function(control, column, index, value)
		return (value.folder and "^xFFFF77" or "^7") .. value.label
	end
	c.folders.GetDragValue = function(control, index, value)
		return "Entry", value
	end
	c.folders.CanDragToValue = function(control, index, value, source)
		return value.folder and source.selValue ~= value
	end
	c.folders.ReceiveDrag = function(control, type, value, source)
		control.lastEvent = "into " .. (control.hoverValue and control.hoverValue.label or "-") .. ": " .. value.label
	end
	c.folders.dragTargetList = { c.folders }
	c.path = new("PathControl"):PathControl(nil, {20, 270, 520, 24}, "Builds/", "One/Two/", function(subPath)
		self.lastPath = subPath
	end)
	c.path.ReceiveDrag = function(control, type, value, source)
		control.lastEvent = "path got " .. value.label
	end
	t_insert(c.src.dragTargetList, c.dst)
	t_insert(c.src.dragTargetList, c.reject)
	t_insert(c.src.dragTargetList, c.folders)
	t_insert(c.src.dragTargetList, c.path)
	t_insert(c.dst.dragTargetList, c.src)
	c.status = new("LabelControl"):LabelControl(nil, {20, 310, 0, 16}, function()
		return string.format("^7src: %s (%d) | dst: %s (%d)", c.src.lastEvent or "-", #c.src.list, c.dst.lastEvent or "-", #c.dst.list)
	end)
	c.status2 = new("LabelControl"):LabelControl(nil, {20, 330, 0, 16}, function()
		return string.format("^7folders: %s | path: %s %s | sel %s", c.folders.lastEvent or "-", c.path.lastEvent or "-", self.lastPath or "-", tostring(c.src.selIndex))
	end)
end

function G:OnFrame(inputEvents, viewPort)
	self.host:ProcessControlsInput(inputEvents, viewPort)
	SetDrawColor(0.1, 0.1, 0.1)
	DrawImage(nil, viewPort.x, viewPort.y, viewPort.width, viewPort.height)
	self.host:DrawControls(viewPort)
	-- Modules/Main.lua:445-454 (main:OnFrame does not run in a gallery)
	if main.showDragText then
		local cursorX, cursorY = GetCursorPos()
		local strWidth = DrawStringWidth(16, "VAR", main.showDragText)
		SetDrawLayer(20, 0)
		SetDrawColor(0.15, 0.15, 0.15, 0.75)
		DrawImage(nil, cursorX, cursorY - 8, strWidth + 2, 18)
		SetDrawColor(1, 1, 1)
		DrawString(cursorX + 1, cursorY - 7, "LEFT", 16, "VAR", main.showDragText)
		main.showDragText = nil
	end
end

return G
