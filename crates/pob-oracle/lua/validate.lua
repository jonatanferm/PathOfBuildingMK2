-- pob-oracle: syntax check for patched PoB sources (src/patcher.rs).
--
-- Returns `check(src, chunkname) -> true | nil, err`: the source goes
-- through the same compound-assignment rewrite the oracle's file loader
-- applies (crates/pob-oracle/src/jit.rs PRELUDE: PoB's LuaJIT accepts
-- `n += 1`) and is then compiled with `loadstring`, never run.

local compound_ops = { ["+"] = true, ["-"] = true, ["*"] = true, ["/"] = true, ["%"] = true, [".."] = true }
local function rewrite_line(line)
	local ind, lhs, op, rhs = line:match("^([ \t]*)([%a_][%w_%.%[%]\"']*)[ \t]*([%+%-%*/%%%.]%.?)=[ \t]*(.-)[ \t]*\r?$")
	if ind and compound_ops[op] and rhs ~= "" then
		local code, comment = rhs:match("^(.-)[ \t]*(%-%-.*)$")
		if code then rhs = code else comment = "" end
		return ind .. lhs .. " = " .. lhs .. " " .. op .. " (" .. rhs .. ")" .. (comment ~= "" and (" " .. comment) or "")
	end
	return line
end

return function(src, chunkname)
	if src:sub(1, 1) == "#" then src = "--" .. src end
	if src:find("[%+%-%*/%%%.]=") then
		src = src:gsub("[^\n]+", rewrite_line)
	end
	local fn, err = loadstring(src, chunkname)
	if not fn then return nil, err end
	return true
end
