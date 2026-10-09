-- pob-dump walker: canonical JSON encoding of Lua values.
--
-- See crates/pob-dump/FORMAT.md for the encoding specification. In short:
--   * a table whose keys are exactly 1..n (n >= 1) and that has no metatable
--     is a JSON array;
--   * every other table is a JSON object: valid-UTF-8 string keys map
--     directly (a key starting with "$" is written with one more "$"),
--     plus the reserved members "$seq" (values at 1..n), "$num" (other
--     numeric keys, canonical number text -> value), "$keys" (any other key
--     type, as [key, value] pairs) and "$meta" (the metatable);
--   * shared tables are emitted once, at their canonical path (breadth-first
--     shortest path, ties broken by key order), and as {"$ref": path}
--     everywhere else -- this also breaks cycles;
--   * functions become {"$fn": id, "src": "file:first-last", "hash": sha256};
--   * numbers: integral values in (-2^53, 2^53) are written as integers,
--     everything else with the shortest of %.15g/%.16g/%.17g that
--     round-trips; non-finite and negative zero as {"$float": "..."};
--   * strings that are not valid UTF-8 become {"$bytes": "<hex>"}.
local W = {}

local type, next, rawget, tostring, tonumber = type, next, rawget, tostring, tonumber
-- Raw metatable / function info come from Rust (the debug library is not
-- loaded in a safe mlua state).
local raw_getmetatable = __dump_getmetatable
local lua_getmetatable = getmetatable
-- Plain getmetatable is nil exactly when there is no metatable; only then
-- is the (slower) raw lookup needed to see past a __metatable field.
local function getmetatable(t)
    if lua_getmetatable(t) == nil then return nil end
    return raw_getmetatable(t)
end
local fninfo = __dump_fninfo
local fmt = string.format
local floor = math.floor
local huge = math.huge
local t_sort, t_concat = table.sort, table.concat

local utf8_ok = __dump_utf8_ok
local fn_hash = __dump_fn_hash
local root_prefix = __dump_root

-- value (table/function) -> canonical path. Strong keys (a huge weak table
-- makes every GC cycle expensive); resetCanon() starts a new, independent
-- group of roots (refs never cross groups) and releases the old one.
local canon = {}
function W.resetCanon()
    canon = {}
end
-- function -> record; list of records in discovery order.
local fnRec = setmetatable({}, { __mode = "k" })
W.fns = {}
W.stats = { tables = 0, refs = 0, fns = 0, opaque = 0, bytes = 0 }
W.pretty = true

local KEYWORDS = {}
for w in ("and break do else elseif end false for function goto if in local nil not or repeat return then true until while"):gmatch("%a+") do
    KEYWORDS[w] = true
end

local function strOk(s)
    return not s:find("[\128-\255]") or utf8_ok(s)
end

local escMap = { ['"'] = '\\"', ['\\'] = '\\\\', ['\b'] = '\\b', ['\f'] = '\\f', ['\n'] = '\\n', ['\r'] = '\\r', ['\t'] = '\\t' }
local function escChar(c)
    return escMap[c] or fmt("\\u%04x", c:byte())
end
local function jstr(s)
    return '"' .. (s:gsub('[%c"\\]', escChar)) .. '"'
end
W.jstr = jstr

local function hex(s)
    return (s:gsub(".", function(c) return fmt("%02x", c:byte()) end))
end

-- Canonical text of a finite number (no JSON wrapping).
local function numText(v)
    if v == floor(v) and v > -9007199254740992 and v < 9007199254740992 then
        if v == 0 and 1 / v < 0 then return nil end
        return fmt("%d", v)
    end
    local s
    for _, f in ipairs({ "%.15g", "%.16g", "%.17g" }) do
        s = fmt(f, v)
        if tonumber(s) == v then return s end
    end
    return s
end
W.numText = numText

local function numJson(v)
    if v ~= v then return '{"$float":"nan"}' end
    if v == huge then return '{"$float":"inf"}' end
    if v == -huge then return '{"$float":"-inf"}' end
    local s = numText(v)
    if not s then return '{"$float":"-0"}' end
    return s
end

-- Text used for a numeric key, both in "$num" and in paths.
local function numKey(k)
    if k ~= k then return "nan" end
    if k == huge then return "inf" end
    if k == -huge then return "-inf" end
    return numText(k) or "-0"
end

local function strKeyPath(k)
    if k:match("^[%a_][%w_]*$") and not KEYWORDS[k] then
        return "." .. k
    end
    return "[" .. jstr(k) .. "]"
end
W.strKeyPath = strKeyPath

-- Split the keys of t into: n (1..n present), sorted valid-UTF-8 string
-- keys, sorted other numeric keys, and every other key.
local function classify(t)
    local n = 0
    while rawget(t, n + 1) ~= nil do n = n + 1 end
    local sk, nk, ok = {}, {}, nil
    for k in next, t do
        local tk = type(k)
        if tk == "string" then
            if strOk(k) then
                sk[#sk + 1] = k
            else
                ok = ok or {}
                ok[#ok + 1] = k
            end
        elseif tk == "number" then
            if not (k >= 1 and k <= n and k == floor(k)) then nk[#nk + 1] = k end
        else
            ok = ok or {}
            ok[#ok + 1] = k
        end
    end
    t_sort(sk)
    t_sort(nk)
    return n, sk, nk, ok
end

-- Path component and sort key for a key that is not a valid-UTF-8 string or
-- a number. Tables and functions used as keys must already have a canonical
-- path (they are referenced by it); anything else is an error so the
-- problem is visible instead of silently non-deterministic.
local function otherKeyText(k, where)
    local tk = type(k)
    if tk == "boolean" then return tostring(k) end
    if tk == "string" then return "$bytes:" .. hex(k) end
    if (tk == "table" or tk == "function") and canon[k] then return "@" .. canon[k] end
    error("pob-dump: unsupported table key of type " .. tk .. " at " .. where)
end

local function sortedOther(ok, where)
    if not ok then return nil end
    local list = {}
    for _, k in ipairs(ok) do list[#list + 1] = { otherKeyText(k, where), k } end
    t_sort(list, function(a, b) return a[1] < b[1] end)
    return list
end

local function registerFn(f, path)
    local rec = fnRec[f]
    if rec then
        rec.aliases[#rec.aliases + 1] = path
        return
    end
    local what, source, l1, l2 = fninfo(f)
    rec = { id = path, aliases = {}, what = what, l1 = l1 or -1, l2 = l2 or -1 }
    source = source or "?"
    if what ~= "C" and source:sub(1, 1) == "@" then
        local file = source:sub(2)
        rec.file = file
        if file:sub(1, #root_prefix) == root_prefix then
            rec.rel = file:sub(#root_prefix + 1)
        else
            rec.rel = file
        end
        rec.hash = fn_hash(file, rec.l1, rec.l2)
        rec.src = fmt("%s:%d-%d", rec.rel, rec.l1, rec.l2)
    elseif what == "C" then
        rec.src = "[C]"
        rec.hash = fn_hash("", 0, 0, tostring(path))
    else
        rec.src = "[string]:" .. tostring(rec.l1) .. "-" .. tostring(rec.l2)
        rec.hash = fn_hash("", 0, 0, source)
    end
    rec.json = '{"$fn":' .. jstr(path) .. ',"src":' .. jstr(rec.src) .. ',"hash":"' .. rec.hash .. '"}'
    fnRec[f] = rec
    W.fns[#W.fns + 1] = rec
    W.stats.fns = W.stats.fns + 1
end

-- Breadth-first canonical path assignment from a root.
function W.assign(root, rootPath)
    local tr = type(root)
    if tr == "function" then
        if not canon[root] then canon[root] = rootPath; registerFn(root, rootPath) end
        return
    end
    if tr ~= "table" or canon[root] then return end
    canon[root] = rootPath
    local queue, paths, head = { root }, { rootPath }, 1
    local function visit(v, path)
        local tv = type(v)
        if tv == "table" then
            if canon[v] == nil then
                canon[v] = path
                queue[#queue + 1] = v
                paths[#paths + 1] = path
            end
        elseif tv == "function" then
            if canon[v] == nil then
                canon[v] = path
                registerFn(v, path)
            else
                registerFn(v, path)
            end
        end
    end
    -- Entries under keys that are not strings/numbers (e.g. data.gemForSkill,
    -- keyed by granted-effect tables) are visited only after the ordinary
    -- breadth-first pass, so key tables get their canonical path from where
    -- they are stored as values; their own entries come last.
    local deferred, dhead = {}, 1
    while true do
        while queue[head] do
            local t, p = queue[head], paths[head]
            queue[head], paths[head] = false, false
            head = head + 1
            local n, sk, nk, ok = classify(t)
            for _, k in ipairs(sk) do visit(rawget(t, k), p .. strKeyPath(k)) end
            for i = 1, n do visit(rawget(t, i), p .. "[" .. i .. "]") end
            for _, k in ipairs(nk) do visit(rawget(t, k), p .. "[" .. numKey(k) .. "]") end
            local mt = getmetatable(t)
            if mt ~= nil then visit(mt, p .. "<meta>") end
            if ok then deferred[#deferred + 1] = { t, p, ok } end
        end
        if not deferred[dhead] then break end
        while deferred[dhead] do
            local t, p, ok = deferred[dhead][1], deferred[dhead][2], deferred[dhead][3]
            deferred[dhead] = false
            dhead = dhead + 1
            for _, e in ipairs(sortedOther(ok, p)) do visit(rawget(t, e[2]), p .. "[" .. e[1] .. "]") end
        end
    end
end

local function joinContainer(open, close, items, indent)
    local total, multiline = 2, false
    for i = 1, #items do
        local s = items[i]
        total = total + #s + 1
        if total > 100 or s:find("\n", 1, true) then multiline = true; break end
    end
    if not W.pretty or not multiline then
        return open .. t_concat(items, ",") .. close
    end
    local ind = "\n" .. string.rep(" ", indent + 1)
    return open .. ind .. t_concat(items, "," .. ind) .. "\n" .. string.rep(" ", indent) .. close
end

local function escKey(k)
    if k:sub(1, 1) == "$" then k = "$" .. k end
    return jstr(k)
end

local encode
-- Encode v found at `path`, `indent` levels deep.
encode = function(v, path, indent)
    local tv = type(v)
    if tv == "string" then
        if strOk(v) then return jstr(v) end
        W.stats.bytes = W.stats.bytes + 1
        return '{"$bytes":"' .. hex(v) .. '"}'
    elseif tv == "number" then
        return numJson(v)
    elseif tv == "boolean" then
        return v and "true" or "false"
    elseif tv == "nil" then
        return "null"
    elseif tv == "function" then
        local rec = fnRec[v]
        if not rec then error("pob-dump: function at " .. path .. " was not assigned") end
        return rec.json
    elseif tv == "table" then
        local c = canon[v]
        if c == nil then error("pob-dump: table at " .. path .. " was not assigned") end
        if c ~= path then
            W.stats.refs = W.stats.refs + 1
            return '{"$ref":' .. jstr(c) .. '}'
        end
        W.stats.tables = W.stats.tables + 1
        local n, sk, nk, ok = classify(v)
        local mt = getmetatable(v)
        local d = indent + 1
        if n > 0 and #sk == 0 and #nk == 0 and not ok and mt == nil then
            local items = {}
            for i = 1, n do items[i] = encode(rawget(v, i), path .. "[" .. i .. "]", d) end
            return joinContainer("[", "]", items, indent)
        end
        local items = {}
        for _, k in ipairs(sk) do
            items[#items + 1] = escKey(k) .. ":" .. encode(rawget(v, k), path .. strKeyPath(k), d)
        end
        if n > 0 then
            local seq = {}
            for i = 1, n do seq[i] = encode(rawget(v, i), path .. "[" .. i .. "]", d + 1) end
            items[#items + 1] = '"$seq":' .. joinContainer("[", "]", seq, d)
        end
        if #nk > 0 then
            local num = {}
            for _, k in ipairs(nk) do
                local kt = numKey(k)
                num[#num + 1] = '"' .. kt .. '":' .. encode(rawget(v, k), path .. "[" .. kt .. "]", d + 1)
            end
            items[#items + 1] = '"$num":' .. joinContainer("{", "}", num, d)
        end
        if ok then
            local pairs_ = {}
            for _, e in ipairs(sortedOther(ok, path)) do
                local k = e[2]
                local kj
                if type(k) == "string" then
                    kj = '{"$bytes":"' .. hex(k) .. '"}'
                else
                    kj = encode(k, path .. "{key}", d + 2)
                end
                pairs_[#pairs_ + 1] = "[" .. kj .. "," .. encode(rawget(v, k), path .. "[" .. e[1] .. "]", d + 2) .. "]"
            end
            items[#items + 1] = '"$keys":' .. joinContainer("[", "]", pairs_, d)
        end
        if mt ~= nil then
            items[#items + 1] = '"$meta":' .. encode(mt, path .. "<meta>", d)
        end
        return joinContainer("{", "}", items, indent)
    else
        W.stats.opaque = W.stats.opaque + 1
        return '{"$opaque":' .. jstr(tv) .. '}'
    end
end
W.encode = function(v, path) return encode(v, path, 0) end

-- ModCache.lua: c[line] = { modList|nil, extraText|nil }. Emitted as
-- {"<line>": {"mods": <modList>|null, "extra": <string>|null}}.
function W.encodeModCache(c, path)
    local keys = {}
    for k in next, c do keys[#keys + 1] = k end
    t_sort(keys)
    local items = {}
    for i, k in ipairs(keys) do
        local e = c[k]
        if type(k) ~= "string" or not strOk(k) or type(e) ~= "table" then
            error("pob-dump: unexpected ModCache entry " .. tostring(k))
        end
        local p = path .. strKeyPath(k)
        items[i] = escKey(k) .. ': {"mods":' .. encode(rawget(e, 1), p .. "[1]", 2) .. ',"extra":' .. encode(rawget(e, 2), p .. "[2]", 2) .. "}"
    end
    return "{\n " .. t_concat(items, ",\n ") .. "\n}", #keys
end

-- The function manifest, as a Lua array of plain records.
function W.manifest()
    local out = {}
    for i, rec in ipairs(W.fns) do
        out[i] = { id = rec.id, src = rec.src, hash = rec.hash, what = rec.what, aliases = rec.aliases }
    end
    return out
end

return W
