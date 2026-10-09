local M = {}
function M.big(p1, p2, unused)
	local a = p1
	local b = 2
	local function helper(v)
		return v + a
	end
	-- segment 2 starts on the next line
	local c = helper(b)
	local d = 0
	for _, item in ipairs(p2) do
		local e = item * c
		local g = e + 1
		-- loop split here
		d = d + e
	end
	local p2 = 5
	return d + p2
end
function M.shadow(o, k)
	local o = o or {}
	local q = 1
	-- segment 2
	return o, q, k
end
function M.bad(x)
	if x then
		local y = 1
		return y
	end
end
return M
