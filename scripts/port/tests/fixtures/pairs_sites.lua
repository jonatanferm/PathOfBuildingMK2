local pairs = pairs
local t_insert = table.insert
local spec = { allocNodes = {} }
for id, node in pairs(spec.allocNodes) do end
for k, v in pairs({ "a", "b" }) do end
for k, v in pairs({ a = 1, b = 2 }) do end
local seen = {}
local key = {}
seen[key] = true
for obj in pairs(seen) do print(obj.name) end
for name in pairs(cfgs) do if name:match("x") then end end
for i in pairs(t) do local z = i + 1 end
for k, v in next, tbl do end
local list = {}
t_insert(list, 1)
for _, v in pairs(list) do end
for k in pairs(mystery) do end
if next(list) then end
