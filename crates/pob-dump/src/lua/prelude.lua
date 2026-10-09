-- pob-dump prelude: root file access at __dump_root (PoB's src/ directory),
-- accept PoB's LuaJIT compound assignment syntax, stub the optional C
-- modules, and record every file loaded (so the dumper can tell which Data
-- files PoB's own load sequence reached).
--
-- Adapted from crates/pob-calc/src/jit.rs (PRELUDE).
local root = __dump_root
local function abs(p)
    if type(p) ~= "string" or p:sub(1, 1) == "/" or p:match("^%a:[/\\]") then return p end
    if p:sub(1, 2) == "./" then p = p:sub(3) end
    return root .. p
end
__dump_abs = abs
__dump_loaded = {}
package.path = root .. "?.lua;" .. root .. "../runtime/lua/?.lua;" .. root .. "../runtime/lua/?/init.lua"
package.cpath = ""
local raw_open, raw_lines = io.open, io.lines
io.open = function(path, mode)
    if mode then mode = mode:gsub("([rwa])b%+", "%1+b") end
    if mode and mode:find("[wa+]") then
        error("pob-dump: refusing to open '" .. tostring(path) .. "' for writing")
    end
    return raw_open(abs(path), mode)
end
io.lines = function(path, ...)
    if path then return raw_lines(abs(path), ...) end
    return raw_lines()
end
-- PoB's own LuaJIT build accepts compound assignment (`n += 1`); rewrite
-- `lhs op= rhs` statements to `lhs = lhs op (rhs)` as files load. The
-- rewrite is line-by-line, so line numbers (debug.getinfo) are unchanged.
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
loadfile = function(path)
    local full = abs(path)
    local f, e = raw_open(full, "rb")
    if not f then return nil, e end
    local src = f:read("*a")
    f:close()
    __dump_loaded[full] = true
    if src:sub(1, 1) == "#" then src = "--" .. src end
    if src:find("[%+%-%*/%%%.]=") then
        src = src:gsub("[^\n]+", rewrite_line)
    end
    return loadstring(src, "@" .. full)
end
dofile = function(path)
    local fn = assert(loadfile(path))
    return fn()
end
package.loaders[2] = function(name)
    local rel = name:gsub("%.", "/")
    local tried = {}
    for pattern in package.path:gmatch("[^;]+") do
        local file = pattern:gsub("%?", rel)
        local f = raw_open(file, "rb")
        if f then
            f:close()
            local fn, e = loadfile(file)
            if not fn then error(e, 2) end
            return fn
        end
        tried[#tried + 1] = "\n\tno file '" .. file .. "'"
    end
    return table.concat(tried)
end
os.remove = function(path) error("pob-dump: os.remove disabled") end
os.rename = function(a, b) error("pob-dump: os.rename disabled") end
package.preload["lua-utf8"] = function()
    local m = {}
    for _, n in ipairs({ "len", "sub", "reverse", "gsub", "find", "match", "gmatch", "upper", "lower", "format", "byte", "char", "rep" }) do
        m[n] = string[n]
    end
    m.offset = function(_, n) return n or 1 end
    m.codepoint = function(s, i) return s:byte(i or 1) end
    return m
end
for _, name in ipairs({ "lcurl", "lcurl.safe", "lfs", "socket", "cjson", "re", "lua-profiler", "lpeg" }) do
    package.preload[name] = function() return false end
end
arg = {}
