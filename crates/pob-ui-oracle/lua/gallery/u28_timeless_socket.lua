-- Gallery "u28_timeless_socket" (U28): the real TimelessJewelSocketControl with a stand-in
-- `build` (spec.nodes, spec.tree.size) and a stand-in socket viewer (PassiveTreeView: its Draw
-- shows the zoom fields it was given). Hovering the closed box and the rows of the dropped list
-- draws the viewer at sublayer 15 for sockets the spec knows (not for id -1 or an unknown id);
-- a second control at the bottom of the screen drops up (the viewer goes below the box).
-- Rust counterpart: src/gallery/u28_timeless_socket.rs. Interactions:
-- u28_timeless_socket.pobscript.

local G = { }

function G:Init()
	local lastSel = "-"
	local build = {
		spec = {
			nodes = {
				[26725] = { x = 300, y = -1200 },
				[54127] = { x = -4500.5, y = 812.25 },
			},
			tree = { size = 25000 },
		},
	}
	local viewer = { }
	function viewer:Draw(build, viewPort, inputEvents)
		SetDrawColor(0.2, 0.3, 0.4)
		DrawImage(nil, viewPort.x, viewPort.y, viewPort.width, viewPort.height)
		DrawString(4, 4, "LEFT", 14, "VAR", string.format("^7zoom %s x %.3f y %.3f inputs %d", tostring(self.zoom), self.zoomX, self.zoomY, #inputEvents))
	end
	local function sockets()
		return {
			{ label = "All", id = -1 },
			{ label = "# Acrobatics: 26725", keystone = "Acrobatics", id = 26725 },
			{ label = "Iron Reflexes: 54127", keystone = "Iron Reflexes", id = 54127 },
			{ label = "Unknown: 999", keystone = "Unknown", id = 999 },
			"A string entry",
		}
	end
	local function selFunc(name)
		return function(index, value)
			lastSel = name .. " " .. index .. " " .. tostring(type(value) == "table" and value.label or value)
		end
	end
	self.host = new("ControlHost"):ControlHost()
	local c = self.host.controls
	c.top = new("TimelessJewelSocketControl"):TimelessJewelSocketControl(nil, {40, 300, 200, 20}, sockets(), selFunc("top"), build, viewer)
	c.bottom = new("TimelessJewelSocketControl"):TimelessJewelSocketControl(nil, {700, 660, 200, 20}, sockets(), selFunc("bottom"), build, viewer)
	c.status = new("LabelControl"):LabelControl(nil, {20, 10, 0, 16}, function()
		return "^7" .. lastSel .. " | top " .. c.top.selIndex .. " bottom " .. c.bottom.selIndex
	end)
end

function G:OnFrame(inputEvents, viewPort)
	self.host:ProcessControlsInput(inputEvents, viewPort)
	SetDrawColor(0.1, 0.1, 0.1)
	DrawImage(nil, viewPort.x, viewPort.y, viewPort.width, viewPort.height)
	self.host:DrawControls(viewPort)
end

return G
