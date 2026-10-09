#!/usr/bin/env lua
-- Scope-resolution fixture for luaast (line numbers are asserted by test_luaast.py).
local x = 1
local x = x + 1          -- RHS x is the line-3 x
local function rec(n)    -- rec is visible in its own body
	if n > 0 then return rec(n - 1) end
	return n
end
local f = function() return f end   -- this f is the global f
for i = 1, 3 do
	local y = i
end
print(i, y)              -- both global here
repeat
	local z = 1
until z == 1             -- until sees the body's z
local obj = {}
function obj:method(a)
	return self, a
end
function obj.plain(b) return b end
do
	local x = "inner"
	goto done
	::done::
end
undeclaredGlobal = x
local t = { a = 1, ["b"] = 2, 3, [x] = 4 }
local s = [==[
long ]] string
]==] .. "\x41\65\z
          B" .. '\'q\''
--[[ long
comment ]] local after = 0x1F + 1e3 + .5 + 0x1p4 + 3LL
count += 1
local a, b = b, a        -- RHS b and a are globals
return t.a, t["b"], s, after, a, b
