-- pob-ui-oracle: creation order, canonical child order and control-path markers
-- (ui doc D7, §11.2 patches 6 and 7). Loaded by markers.rs before PoB's own files.
--
-- * __stampSeq(object)      called from the patched Common.lua `new()`: remembers the creation
--                           sequence number of every object (weak-keyed side table, so no field
--                           is added to PoB's objects and `pairs(object)` is unchanged).
-- * __canonPairs(t)         replaces `pairs(self.controls)` at ControlHost.lua:32, :110 and
--                           Main.lua:1298, :1308. Integer keys ascending, then the other keys by
--                           the creation sequence of their value (ties and values that were not
--                           made by `new()`: by tostring(key)). The keys are snapshotted when the
--                           loop starts; a key whose value became nil before it is reached is
--                           skipped (what Lua `pairs` does for an entry removed during traversal).
-- * __tracePush(host, key)  / __tracePop(): the control path "<HostClass>.<key>" around
--                           `control:Draw` in the patched DrawControls loops (Rust:
--                           Cx::draw_controls -> Cx::trace_push).
-- * Audit mode (`__oracleAudit = true`): __canonPairs yields the native `pairs` order, and every
--   drawn control is recorded with its rectangle; __auditEndFrame() lists hosts whose native draw
--   order differs from the canonical one between two overlapping controls.

local rawget, type, tostring, next, pairs = rawget, type, tostring, next, pairs
local t_sort = table.sort

local seqOf = setmetatable({ }, { __mode = "k" })
local nextSeq = 0

function __stampSeq(object)
	nextSeq = nextSeq + 1
	seqOf[object] = nextSeq
end

function __seqOf(object)
	return seqOf[object]
end

local function canonKeys(t)
	local nums, others = { }, { }
	for k in next, t do
		if type(k) == "number" then
			nums[#nums + 1] = k
		else
			others[#others + 1] = k
		end
	end
	t_sort(nums)
	t_sort(others, function(a, b)
		local sa, sb = seqOf[rawget(t, a)], seqOf[rawget(t, b)]
		if sa and sb and sa ~= sb then
			return sa < sb
		elseif sa and not sb then
			return true
		elseif sb and not sa then
			return false
		end
		return tostring(a) < tostring(b)
	end)
	for i = 1, #others do
		nums[#nums + 1] = others[i]
	end
	return nums
end
__canonKeys = canonKeys

function __canonPairs(t)
	if __oracleAudit then
		return pairs(t)
	end
	local keys = canonKeys(t)
	local i = 0
	return function()
		while true do
			i = i + 1
			local k = keys[i]
			if k == nil then
				return nil
			end
			local v = rawget(t, k)
			if v ~= nil then
				return k, v
			end
		end
	end
end

-- Control-path markers.
local stack = { }
local push, pop = __ui_ctl_push, __ui_ctl_pop

-- Audit state: per frame, per host table, the drawn controls in native order.
local auditFrame = { }
local auditHosts = { }
__auditHits = { }

local function hostClass(host)
	local c = host._className
	return c and tostring(c) or "?"
end

function __tracePush(host, key)
	local elem = hostClass(host) .. "." .. tostring(key)
	if __oracleAudit then
		local list = auditFrame[host]
		if not list then
			list = { path = table.concat(stack, "/"), class = hostClass(host) }
			auditFrame[host] = list
			auditHosts[#auditHosts + 1] = host
		end
		local control = rawget(host.controls, key)
		local ok, x, y = pcall(control.GetPos, control)
		local ok2, w, h = pcall(control.GetSize, control)
		list[#list + 1] = { key = key, control = control, x = ok and x or 0, y = ok and y or 0, w = ok2 and w or 0, h = ok2 and h or 0 }
	end
	stack[#stack + 1] = elem
	push(elem)
end

function __tracePop()
	stack[#stack] = nil
	pop()
end

local function overlaps(a, b)
	return a.x < b.x + b.w and b.x < a.x + a.w and a.y < b.y + b.h and b.y < a.y + a.h
end

-- Compare each host's native draw order with the canonical one; record inverted overlapping pairs.
function __auditEndFrame()
	for _, host in ipairs(auditHosts) do
		local list = auditFrame[host]
		if #list >= 2 then
			local rank = { }
			local keys = canonKeys(host.controls)
			for i, k in ipairs(keys) do
				rank[k] = i
			end
			for i = 1, #list - 1 do
				for j = i + 1, #list do
					local a, b = list[i], list[j]
					local ra, rb = rank[a.key], rank[b.key]
					if ra and rb and ra > rb and overlaps(a, b) then
						local label = (list.path ~= "" and (list.path .. "/") or "") .. list.class
						local hit = __auditHits[label]
						if not hit then
							hit = { count = 0, example = tostring(b.key) .. " < " .. tostring(a.key) }
							__auditHits[label] = hit
						end
						hit.count = hit.count + 1
					end
				end
			end
		end
	end
	auditFrame = { }
	auditHosts = { }
end
