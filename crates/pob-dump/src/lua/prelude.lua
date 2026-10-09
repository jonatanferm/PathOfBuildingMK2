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
-- In-memory source hooks, keyed by path relative to the PoB src dir
-- (e.g. "Modules/ModParser.lua"): hook(src) -> src. Used by the parse
-- oracle to instrument ModParser; hooks must keep every line number.
-- Nothing on disk is changed.
__dump_src_hooks = {}
-- Negative zero literals. PoB's reference LuaJIT (x86-64, single-number
-- mode) reads the literal `-0` (Data/ModCache.lua: `value=-0`) as the double
-- -0. A dual-number LuaJIT (every ARM64 build; the vendored luajit-src
-- 210.5.12 also on unary minus of an integer 0) folds it to the integer 0,
-- losing the sign. When this LuaJIT does, a `=-0` followed by `,` or `}`
-- (a table field value) is rewritten to `=(0/-1)`, which is -0 on every
-- build; the count per file is recorded. Line numbers are unchanged.
__dump_negzero_lost = 1 / loadstring("return -0")() > 0
__dump_negzero_rewrites = {}
local function fix_negzero(rel, src)
    if not __dump_negzero_lost or not src:find("=-0", 1, true) then return src end
    local out, n = src:gsub("=%-0([,}])", "=(0/-1)%1")
    if n > 0 then __dump_negzero_rewrites[rel] = (__dump_negzero_rewrites[rel] or 0) + n end
    return out
end
loadfile = function(path)
    local full = abs(path)
    local f, e = raw_open(full, "rb")
    if not f then return nil, e end
    local src = f:read("*a")
    f:close()
    __dump_loaded[full] = true
    local rel = full:sub(1, #root) == root and full:sub(#root + 1) or full
    local hook = __dump_src_hooks[rel]
    if hook then src = hook(src) end
    src = fix_negzero(rel, src)
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
