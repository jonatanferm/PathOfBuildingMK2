-- U4 T0 class shim (ui doc §11.1 T0): the parts of Modules/Common.lua that the REAL Control.lua,
-- ControlHost.lua and UndoHandler.lua need, loaded into mlua/LuaJIT before them. The test
-- harness also loads Common.lua's own wipeTable/isValueInArray/isMouseInRegion text verbatim.
-- `new` stamps a creation seq and `__canonPairs` is the oracle's canonical iteration (§11.2
-- patch 6, D7), which the harness substitutes for `pairs(self.controls)` at ControlHost.lua:32/:110.
common = { classes = { } }
local seq = 0
function getClass(name) return common.classes[name] end
function newClass(className, ...)
	local class = { _className = className }
	common.classes[className] = class
	class.__index = class
	local parents = { ... }
	if #parents > 0 then
		setmetatable(class, { __index = function(_, key)
			for _, parent in ipairs(parents) do
				local val = getClass(parent)[key]
				if val ~= nil then return val end
			end
		end })
	end
	return class
end
function new(className)
	seq = seq + 1
	local object = setmetatable({ __seq = seq }, getClass(className))
	object.Object = object
	return object
end
function __canonPairs(t)
	local keys = { }
	for k in pairs(t) do keys[#keys + 1] = k end
	table.sort(keys, function(a, b)
		local na, nb = type(a) == "number", type(b) == "number"
		if na ~= nb then return na end
		if na then return a < b end
		return t[a].__seq < t[b].__seq
	end)
	local i = 0
	return function()
		repeat i = i + 1 until keys[i] == nil or t[keys[i]] ~= nil
		if keys[i] ~= nil then return keys[i], t[keys[i]] end
	end
end
CURSOR_X, CURSOR_Y = 0, 0
function GetCursorPos() return CURSOR_X, CURSOR_Y end
