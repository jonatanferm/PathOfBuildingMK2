-- pob-dump parse oracle: run PoB's own modLib.parseMod (Modules/ModParser.lua,
-- loaded through Modules/ModTools exactly as Modules/Main.lua loads it) over
-- every mod line PoB parses, and record for each distinct line what PoB
-- returns plus which pattern won every scan() call.
--
-- ModParser.lua is instrumented in memory only (a loadfile source hook, see
-- prelude.lua); every rewrite keeps all line numbers, and the file on disk
-- is untouched. The rewrites:
--   * `local function scan(line, patternList, plain)` (ModParser.lua:6616)
--     gets ` do return __pd_scan(line, patternList, plain) end` appended:
--     __pd_scan is the same algorithm (lowest start, then longest end, then
--     longest pattern), iterating patterns in sorted key order, and records
--     the winner. On a full tie (same start, end and pattern length) PoB's
--     result depends on pairs() order, i.e. is random; __pd_scan picks the
--     candidate with the lowest ModParser.lua line (calc-architecture.md
--     section 3.4; then the bytewise-smallest pattern), which is one of the
--     outcomes PoB itself can produce, and records every candidate.
--   * `local function parseMod(line, order)` (6640) gets ` __pd_pass(line, order)`
--     appended, which opens a pass record and records the jewelFuncList /
--     clusterJewelSkills / unsupportedModList early exits (6641-6656).
--   * top-level writes into the pattern tables before scan() (keystones,
--     the specialModList "^...$" wrap, skill-name loops, jewelFuncList, ...)
--     become `__pd_track("<name>", <name>, <line>)[k] = v`, which records
--     the line that wrote each generated key.
--   * `local cache = { }` (6994) gets ` __pd_lists({ ... })` appended, which
--     hands the pattern tables (locals) to the oracle.
-- Literal table entries get their line from the source text.

local PO = {}

local fmt, t_sort, t_concat = string.format, table.sort, table.concat
local next, type, tostring, pcall, ipairs = next, type, tostring, pcall, ipairs
local huge = math.huge
local utf8_ok = __dump_utf8_ok

-- Pattern tables of ModParser.lua (locals), by their source names.
local LISTS = {
    "formList", "modNameList", "modFlagList", "preFlagList", "modTagList", "specialModList",
    "unsupportedModList", "clusterJewelSkills", "suffixTypes", "penTypes", "resourceTypes",
    "regenTypes", "degenTypes", "costTypes", "baseCostTypes", "flagTypes", "skillNameList",
    "preSkillNameList", "jewelOtherFuncs", "jewelSelfFuncs", "jewelSelfUnallocFuncs",
    "jewelThresholdFuncs", "jewelFuncList", "dmgTypes",
}
local isList = {}
for _, n in ipairs(LISTS) do isList[n] = true end
-- appendMod(resourceTypes, ...) copies (ModParser.lua:6036-6052): same keys.
local APPEND_OF = { regenTypes = "resourceTypes", degenTypes = "resourceTypes", costTypes = "resourceTypes", baseCostTypes = "resourceTypes" }
-- jewelFuncList entry type -> table its key came from (ModParser.lua:6552-6594)
local JEWEL_SRC = { Other = "jewelOtherFuncs", Self = "jewelSelfFuncs", SelfUnalloc = "jewelSelfUnallocFuncs", Threshold = "jewelThresholdFuncs" }

local lit = {}      -- name -> key -> line of the literal entry
local litLower = {} -- name -> key:lower() -> line (jewel*Funcs)
local genBy = {}    -- table -> key -> line of the generating statement
local nameOf = {}   -- table -> name
local L             -- name -> table, from __pd_lists
local lines = {}    -- the rewritten target lines (for the manifest)
local wrapLine

----------------------------------------------------------------------------
-- Source instrumentation

-- Decode the Lua string literal at the start of `text` if it is followed by
-- `]` and `=` (a `["key"] = value` table entry). Returns nil otherwise.
local function parseKeyLiteral(text)
    local q = text:sub(1, 1)
    if q ~= '"' and q ~= "'" then return nil end
    local i = 2
    while true do
        local c = text:sub(i, i)
        if c == "" then return nil end
        if c == "\\" then
            i = i + 2
        elseif c == q then
            break
        else
            i = i + 1
        end
    end
    if not text:sub(i + 1):match("^%s*%]%s*=") then return nil end
    local f = loadstring("return " .. text:sub(1, i))
    if not f then return nil end
    return f()
end

local function instrument(src)
    local src_lines = {}
    for l in (src .. "\n"):gmatch("([^\n]*)\n") do src_lines[#src_lines + 1] = l end
    local n_lines = #src_lines
    local inLit = {}
    local i = 1
    while i <= n_lines do
        local name = src_lines[i]:match("^local ([%w_]+) = {%s*$")
        if name and isList[name] then
            lit[name] = lit[name] or {}
            local j = i + 1
            while src_lines[j] ~= "}" do
                assert(src_lines[j], "pob-dump parse oracle: unterminated constructor " .. name)
                inLit[j] = true
                -- one indent level: a tab, or (ModParser.lua:5680) four spaces
                local rest = src_lines[j]:match("^\t%[(.*)$") or src_lines[j]:match("^    %[(.*)$")
                if rest then
                    local k = parseKeyLiteral(rest)
                    if k ~= nil then lit[name][k] = j end
                end
                j = j + 1
            end
            i = j + 1
        else
            i = i + 1
        end
    end
    local function once(text, what)
        local found
        for n = 1, n_lines do
            if src_lines[n] == text then
                assert(not found, "pob-dump parse oracle: ModParser.lua has two " .. what)
                found = n
            end
        end
        assert(found, "pob-dump parse oracle: ModParser.lua " .. what .. " not found (upstream changed?)")
        return found
    end
    local scanLine = once("local function scan(line, patternList, plain)", "scan()")
    local parseLine = once("local function parseMod(line, order)", "parseMod()")
    local cacheLine = once("local cache = { }", "cache")
    wrapLine = once('\tspecialModList["^"..k.."$"] = v', "specialModList wrap")
    lines.scan, lines.parseMod, lines.cache, lines.wrap = scanLine, parseLine, cacheLine, wrapLine
    local tracked = 0
    for n = 1, scanLine - 1 do
        if not inLit[n] then
            local ind, name, rest = src_lines[n]:match("^(%s*)([%a_][%w_]*)%[(.*)$")
            if name and isList[name] then
                src_lines[n] = ind .. "__pd_track(\"" .. name .. "\", " .. name .. ", " .. n .. ")[" .. rest
                tracked = tracked + 1
            end
        end
    end
    lines.tracked = tracked
    src_lines[scanLine] = src_lines[scanLine] .. " do return __pd_scan(line, patternList, plain) end"
    src_lines[parseLine] = src_lines[parseLine] .. " __pd_pass(line, order)"
    -- Line 1 is a comment; the chunk's globals go through __pd_env (below).
    src_lines[1] = "setfenv(1, __pd_env) " .. src_lines[1]
    local fields = {}
    for _, name in ipairs(LISTS) do fields[#fields + 1] = name .. " = " .. name end
    fields[#fields + 1] = "oldList = oldList"
    src_lines[cacheLine] = src_lines[cacheLine] .. " __pd_lists({ " .. t_concat(fields, ", ") .. " })"
    for name, map in pairs(lit) do
        if JEWEL_SRC.Other == name or JEWEL_SRC.Self == name or JEWEL_SRC.SelfUnalloc == name or JEWEL_SRC.Threshold == name then
            local low = {}
            for k, line in pairs(map) do low[k:lower()] = line end
            litLower[name] = low
        end
    end
    local out = t_concat(src_lines, "\n")
    local n_out = select(2, out:gsub("\n", "\n"))
    assert(n_out == select(2, src:gsub("\n", "\n")), "pob-dump parse oracle: line count changed")
    return out
end

-- ModParser's global environment: _G, except that `tonumber` returns a
-- floating-point value. PoB's reference LuaJIT (x86-64) has only doubles,
-- so `-tonumber("0")` (e.g. the RED form, ModParser.lua:6767, or
-- ModParser.lua:4343) is -0 there, which Data/ModCache.lua records as
-- `value=-0`. A dual-number LuaJIT (ARM64) returns an integer 0 from
-- tonumber, and its interpreter negates that to +0 (its JIT to -0, so the
-- result even varied between runs). `x / 1` is the same number as a double;
-- on a single-number build it changes nothing.
local raw_tonumber = tonumber
__pd_env = setmetatable({
    tonumber = function(v, base)
        local n = raw_tonumber(v, base)
        if n then return n / 1 end
        return n
    end,
}, { __index = _G, __newindex = _G })

function __pd_track(name, t, line)
    if nameOf[t] == nil then nameOf[t] = name end
    local g = genBy[t]
    if not g then
        g = {}
        genBy[t] = g
    end
    return setmetatable({}, {
        __index = t,
        __newindex = function(_, k, v)
            g[k] = line
            t[k] = v
        end,
    })
end

function __pd_lists(t)
    L = t
    for _, name in ipairs(LISTS) do
        assert(t[name] ~= nil, "pob-dump parse oracle: ModParser.lua local " .. name .. " missing")
        nameOf[t[name]] = name
    end
end

-- ModParser.lua line of `key` in pattern table `t`; second result true when
-- the key was written by a generating statement (a loop) rather than a
-- literal table entry.
local function prov(t, key)
    local name = nameOf[t]
    if name == nil then return nil, false end
    local alias = APPEND_OF[name]
    if alias then
        name = alias
        t = L[alias]
    end
    local g = genBy[t] and genBy[t][key]
    if g then
        if g == wrapLine and type(key) == "string" then
            -- specialModList["^"..k.."$"] = v: report where k came from
            local inner = key:sub(2, -2)
            local g2 = genBy[L.oldList] and genBy[L.oldList][inner]
            if g2 then return g2, true end
            return lit.specialModList[inner], false
        end
        if name == "jewelFuncList" then
            local v = t[key]
            local src = type(v) == "table" and JEWEL_SRC[v.type]
            local l = src and litLower[src] and litLower[src][key]
            if l then return l, false end
        end
        return g, true
    end
    return lit[name] and lit[name][key], false
end
PO.prov = prov

----------------------------------------------------------------------------
-- Recording

local jstr = nil -- set in PO.run (walker's)
local function jstring(s)
    if type(s) == "string" and (not s:find("[\128-\255]") or utf8_ok(s)) then return jstr(s) end
    if type(s) == "string" then
        return '{"$bytes":"' .. (s:gsub(".", function(c) return fmt("%02x", c:byte()) end)) .. '"}'
    end
    return jstr(tostring(s))
end

local numText
local function jval(v)
    local tv = type(v)
    if tv == "string" then return jstring(v) end
    if tv == "number" then return numText(v) or "0" end
    if tv == "boolean" then return v and "true" or "false" end
    return "null"
end

local function capsJson(caps)
    if not caps or caps[1] == nil then return nil end
    local items = {}
    for i = 1, 5 do
        if caps[i] == nil then break end
        items[i] = jval(caps[i])
    end
    return "[" .. t_concat(items, ",") .. "]"
end

local cur        -- record being parsed
local curSteps   -- step list of the current pass
local stats = { ties = 0, unresolved = 0, jewelTies = 0 }

local function stepJson(list, pattern, line, gen, caps, extra)
    local s = '{"list":' .. jstr(list) .. ',"pattern":' .. (pattern == nil and "null" or jstring(pattern))
    if pattern ~= nil then
        s = s .. ',"line":' .. (line and tostring(line) or "null")
        if line == nil then stats.unresolved = stats.unresolved + 1 end
        if gen then s = s .. ',"gen":true' end
        local c = capsJson(caps)
        if c then s = s .. ',"caps":' .. c end
    end
    if extra then s = s .. extra end
    return s .. "}"
end

local keyCache = setmetatable({}, { __mode = "k" })
local function sortedKeys(t)
    local ks = keyCache[t]
    if not ks then
        ks = {}
        for k in next, t do ks[#ks + 1] = k end
        t_sort(ks)
        keyCache[t] = ks
    end
    return ks
end

-- Index just past the `[set]` starting at p[i] == "[".
local function skipSet(p, i)
    i = i + 1
    if p:sub(i, i) == "^" then i = i + 1 end
    if p:sub(i, i) == "]" then i = i + 1 end
    while i <= #p do
        local c = p:sub(i, i)
        if c == "%" then
            i = i + 2
        elseif c == "]" then
            return i + 1
        else
            i = i + 1
        end
    end
    return i
end

-- The longest piece of literal text every match of Lua pattern `p` must
-- contain (conservative: runs are cut at captures, classes, sets and
-- quantified items), or nil. Used only to skip patterns that cannot match:
-- if `p` matches s, s contains this text, so skipping on a failed plain
-- find never changes scan()'s result.
local function requiredLiteral(p)
    local n = #p
    local best, run = "", ""
    local function flush()
        if #run > #best then best = run end
        run = ""
    end
    local i = 1
    if p:sub(1, 1) == "^" then i = 2 end
    while i <= n do
        local c = p:sub(i, i)
        local lit, nexti
        if c == "(" or c == ")" then
            flush()
            nexti = i + 1
        elseif c == "$" and i == n then
            flush()
            nexti = i + 1
        else
            local single = true
            if c == "%" then
                local d = p:sub(i + 1, i + 1)
                if d == "b" then
                    single, nexti = false, i + 4
                elseif d == "f" then
                    single, nexti = false, skipSet(p, i + 2)
                elseif d:match("%w") then
                    nexti = i + 2 -- class or back reference
                else
                    lit, nexti = d, i + 2
                end
            elseif c == "[" then
                nexti = skipSet(p, i)
            elseif c == "." then
                nexti = i + 1
            else
                lit, nexti = c, i + 1
            end
            if not single then
                flush()
            else
                local q = p:sub(nexti, nexti)
                if q == "*" or q == "-" or q == "?" then
                    flush()
                    nexti = nexti + 1
                elseif q == "+" then
                    if lit then run = run .. lit end
                    flush()
                    nexti = nexti + 1
                elseif lit then
                    run = run .. lit
                else
                    flush()
                end
            end
        end
        i = nexti
    end
    flush()
    if #best >= 2 then return best end
    return nil
end
PO.requiredLiteral = requiredLiteral

PO.prefilter = true
local litCache = setmetatable({}, { __mode = "k" })
-- Per pattern table: pattern -> required literal (false when none).
local function literalsOf(t)
    local m = litCache[t]
    if not m then
        m = {}
        for k in next, t do
            m[k] = type(k) == "string" and requiredLiteral(k) or false
        end
        litCache[t] = m
    end
    return m
end

-- ModParser.lua:6616-6638 scan(), plus recording.
function __pd_scan(line, patternList, plain)
    local keys = sortedKeys(patternList)
    local bestIndex, bestEndIndex
    local bestPattern = ""
    local bestVal, bestStart, bestEnd, bestCaps
    local ties
    local lineLower = line:lower()
    local lits = (PO.prefilter and not plain) and literalsOf(patternList) or nil
    for i = 1, #keys do
        local pattern = keys[i]
        local index, endIndex, cap1, cap2, cap3, cap4, cap5
        local need = lits and lits[pattern]
        if not need or lineLower:find(need, 1, true) then
            index, endIndex, cap1, cap2, cap3, cap4, cap5 = lineLower:find(pattern, 1, plain)
        end
        if index then
            if not bestIndex or index < bestIndex or (index == bestIndex and (endIndex > bestEndIndex or (endIndex == bestEndIndex and #pattern > #bestPattern))) then
                bestIndex = index
                bestEndIndex = endIndex
                bestPattern = pattern
                bestVal = patternList[pattern]
                bestStart = index
                bestEnd = endIndex
                bestCaps = { cap1, cap2, cap3, cap4, cap5 }
                ties = nil
            elseif index == bestIndex and endIndex == bestEndIndex and #pattern == #bestPattern then
                ties = ties or {}
                ties[#ties + 1] = { pattern, { cap1, cap2, cap3, cap4, cap5 } }
            end
        end
    end
    local listName = nameOf[patternList] or "?"
    local bestLine, bestGen
    if bestVal ~= nil or bestIndex then
        bestLine, bestGen = prov(patternList, bestPattern)
    end
    local tieJson
    if ties then
        -- Full tie: PoB keeps whichever comes first in pairs() order.
        -- Choose the lowest source line (section 3.4), keep all candidates.
        stats.ties = stats.ties + 1
        local cands = { { bestPattern, bestCaps, bestLine, bestGen } }
        for _, t in ipairs(ties) do
            local l, g = prov(patternList, t[1])
            cands[#cands + 1] = { t[1], t[2], l, g }
        end
        local items = {}
        for i, c in ipairs(cands) do
            items[i] = '{"pattern":' .. jstring(c[1]) .. ',"line":' .. (c[3] and tostring(c[3]) or "null") .. "}"
        end
        tieJson = ',"ties":[' .. t_concat(items, ",") .. "]"
        local win = cands[1]
        for i = 2, #cands do
            if (cands[i][3] or huge) < (win[3] or huge) then win = cands[i] end
        end
        bestPattern, bestCaps, bestLine, bestGen = win[1], win[2], win[3], win[4]
        bestVal = patternList[bestPattern]
    end
    if curSteps then
        if bestVal ~= nil then
            curSteps[#curSteps + 1] = stepJson(listName, bestPattern, bestLine, bestGen, bestCaps, tieJson)
        else
            curSteps[#curSteps + 1] = stepJson(listName, nil)
        end
    end
    if bestVal then
        return bestVal, line:sub(1, bestStart - 1) .. line:sub(bestEnd + 1, -1), bestCaps
    else
        return nil, line
    end
end

-- Start of ModParser.lua parseMod(line, order): open a pass, and record the
-- early exits at 6641-6656 (evaluated here exactly as parseMod does).
function __pd_pass(line, order)
    if not cur then
        curSteps = nil
        return
    end
    curSteps = {}
    cur.passes[#cur.passes + 1] = { order = order, steps = curSteps }
    local lineLower = line:lower()
    -- 6643-6648: first jewelFuncList pattern (pairs order) with a capture
    local firstPattern, firstCaps, n = nil, nil, 0
    for _, pattern in ipairs(sortedKeys(L.jewelFuncList)) do
        local _, _, cap1, cap2, cap3, cap4, cap5 = lineLower:find(pattern, 1)
        if cap1 then
            n = n + 1
            if not firstPattern then firstPattern, firstCaps = pattern, { cap1, cap2, cap3, cap4, cap5 } end
        end
    end
    if firstPattern then
        local l, g = prov(L.jewelFuncList, firstPattern)
        local extra
        if n > 1 then
            stats.jewelTies = stats.jewelTies + 1
            extra = ',"candidates":' .. n
        end
        curSteps[#curSteps + 1] = stepJson("jewelFuncList", firstPattern, l, g, firstCaps, extra)
        return
    end
    for _, name in ipairs({ "jewelFuncList", "clusterJewelSkills", "unsupportedModList" }) do
        if L[name][lineLower] then
            local l, g = prov(L[name], lineLower)
            curSteps[#curSteps + 1] = stepJson(name, lineLower, l, g, nil, ',"exact":true')
            return
        end
    end
end

----------------------------------------------------------------------------
-- Driver

local recs = {}        -- line -> record
local recCount = 0
local source = "?"
local W                -- walker

local function encodeMods(modList)
    if modList == nil then return "null" end
    W.resetCanon()
    W.assign(modList, "mods")
    return W.encode(modList, "mods")
end

local origParse
local function wrappedParse(line, isComb)
    local rec = recs[line]
    if rec then
        rec.src[source] = true
        if rec.err then return nil end
        return origParse(line, isComb)
    end
    rec = { src = { [source] = true }, passes = {} }
    recs[line] = rec
    recCount = recCount + 1
    cur = rec
    local ok, modList, extra = pcall(origParse, line, isComb)
    cur, curSteps = nil, nil
    if not ok then
        rec.err = tostring(modList)
        return nil
    end
    rec.mods = encodeMods(modList)
    rec.extra = extra
    return modList, extra
end

-- Install the ModParser instrumentation. Must run before Modules/ModTools
-- (which loads Modules/ModParser) is loaded.
function PO.install()
    __dump_src_hooks["Modules/ModParser.lua"] = instrument
    -- Interpreter only. On a dual-number LuaJIT, whether `-num` with an
    -- integer num == 0 (ModParser.lua:4343, "Debuffs on you expire 0%
    -- slower") gave -0 or +0 depended on whether the code had been traced
    -- yet, which varied between runs. With the JIT off and tonumber
    -- returning doubles (__pd_env), every run gives x86-64 PoB's -0.
    if jit then jit.off() end
end

-- Roll positions 0, 0.05, ..., 1. Used as the range argument of
-- itemLib.applyRange for affix-like lines (ItemsTab's affix and range-line
-- sliders are continuous, ItemsTab.lua:934-942, 1077-1082), and as PoB's
-- "default item affix quality" option for uniques (main.defaultItemAffixQuality,
-- a 0-1 slider rounded to 0.01, Main.lua:1136-1137).
local RANGES = {}
for i = 0, 20 do RANGES[#RANGES + 1] = i / 20 end

local function P(line, isComb)
    return modLib.parseMod(line, isComb)
end

-- Parse a list of lines the way Item.lua:1186-1203 does for one modifier:
-- a line that is not fully recognised is retried joined with the next line
-- (isComb), and that line is consumed when the combination parses.
local function parseLineList(list, xform)
    local i = 1
    while list[i] do
        local line = list[i]
        local modList, extra = P(xform(line))
        if (not modList or extra) and list[i + 1] then
            local ml, ex = P(xform(line .. " " .. list[i + 1]), true)
            if ml and not ex then i = i + 1 end
        end
        i = i + 1
    end
end

local function ident(l) return l end

local function sortedPairs(t)
    local ks = {}
    for k in next, t do ks[#ks + 1] = k end
    t_sort(ks, function(a, b)
        if type(a) == type(b) and (type(a) == "string" or type(a) == "number") then return a < b end
        return type(a) < type(b)
    end)
    local i = 0
    return function()
        i = i + 1
        local k = ks[i]
        if k ~= nil then return k, t[k] end
    end
end

-- Mod tables of Data/Mod*.lua: the stat lines are the array part.
local function parseAffixTable(tbl)
    for _, mod in sortedPairs(tbl) do
        if type(mod) == "table" and type(mod[1]) == "string" then
            local list = {}
            for _, l in ipairs(mod) do
                if type(l) == "string" then
                    for s in l:gmatch("[^\n]+") do list[#list + 1] = s end
                end
            end
            for _, r in ipairs(RANGES) do
                parseLineList(list, function(l)
                    local ok, v = pcall(itemLib.applyRange, l, r)
                    return ok and v or l
                end)
            end
        end
        -- Each stat description's lines joined, as Main.lua:205-216 (the
        -- ModCache rebuild, at 0.5) and ConfigModBrowser.lua:84-90 parse them.
        if type(mod) == "table" and type(mod.tradeHashes) == "table" then
            for _, line in sortedPairs(mod.tradeHashes) do
                if type(line) == "table" then
                    local joined = t_concat(line, " ")
                    for _, r in ipairs(RANGES) do
                        local ok, v = pcall(itemLib.applyRange, joined, r)
                        P(ok and v or joined)
                    end
                end
            end
        end
    end
end

-- Every string list under `t` (enchantment tables): "/" separates lines.
local function parseStringLists(t, seen)
    seen = seen or {}
    if seen[t] then return end
    seen[t] = true
    for _, v in sortedPairs(t) do
        if type(v) == "string" then
            local list = {}
            for s in v:gmatch("[^/]+") do list[#list + 1] = s end
            for _, r in ipairs(RANGES) do
                parseLineList(list, function(l)
                    local ok, x = pcall(itemLib.applyRange, l, r)
                    return ok and x or l
                end)
            end
        elseif type(v) == "table" then
            parseStringLists(v, seen)
        end
    end
end

local counts = {}
local errors = {}
local function run(label, fn)
    source = label
    local before = recCount
    local ok, err = xpcall(fn, debug and debug.traceback or function(e) return e end)
    -- new distinct lines first seen in this part, summed per label
    counts[label] = (counts[label] or 0) + (recCount - before)
    if not ok then errors[#errors + 1] = label .. ": " .. tostring(err) end
end

-- opts (all optional):
--   parts: set of corpus parts to run (all when nil): "modCache", "trees",
--          "uniques", "catalysts", "rares", "affixes", "enchantments", "cluster", "bases",
--          "legion", "foulborn", "pantheons";
--   modCacheStride: parse only every n-th ModCache line (tests);
--   prefilter: false disables the literal prefilter (equivalence tests).
function PO.run(walker, opts)
    opts = opts or {}
    W = walker
    jstr = W.jstr
    numText = W.numText
    local parts = opts.parts
    local want = function(p) return parts == nil or parts[p] end
    PO.prefilter = opts.prefilter ~= false
    assert(type(modLib) == "table" and modLib.parseMod, "ModTools not loaded")
    assert(L, "ModParser was not instrumented (PO.install before ModTools)")
    origParse = modLib.parseMod
    modLib.parseMod = wrappedParse
    -- PoB headless logging (ConPrintf -> print) is noise here.
    local savedPrint, savedCon = print, ConPrintf
    print = function() end
    ConPrintf = function() end
    main = main or { defaultItemAffixQuality = 0.5 }

    -- PoB's memoised parses (Data/ModCache.lua keys), parsed natively.
    if want("modCache") then
        run("modCache", function()
            local c = LoadModule("Data/ModCache")
            local keys = {}
            for k in next, c do keys[#keys + 1] = k end
            t_sort(keys)
            local stride = opts.modCacheStride or 1
            for i = 1, #keys, stride do P(keys[i]) end
        end)
    end
    -- Every tree version, through PoB's PassiveTree class (main:LoadTree,
    -- Main.lua:351-363): node, mastery, tattoo and legion stats with
    -- PassiveTree:ProcessStats' line joining.
    if want("trees") then
        for _, ver in ipairs(treeVersionList) do
            run("tree:" .. ver, function()
                data.setJewelRadiiGlobally(ver)
                local tree = new("PassiveTree"):PassiveTree(ver)
                tree = nil
            end)
            collectgarbage()
        end
        data.setJewelRadiiGlobally(latestTreeVersion)
    end
    -- Uniques (every variant's lines) and rares, through PoB's Item class
    -- (Main.lua:163-195 loadItemDBs).
    if want("uniques") then
        for _, q in ipairs(RANGES) do
            main.defaultItemAffixQuality = q
            for type_, list in sortedPairs(data.uniques) do
                run("unique:" .. type_, function()
                    for _, raw in sortedPairs(list) do
                        pcall(function() new("Item"):Item(raw, "UNIQUE", true) end)
                    end
                end)
            end
        end
        main.defaultItemAffixQuality = 0.5
    end
    -- Jewellery uniques with each catalyst at 20% quality (Item.lua:14-60,
    -- 836-848: the "Catalyst:"/"CatalystQuality:" raw lines PoB writes for
    -- catalysed items, Item.lua:1819-1824), at roll positions 0, 0.5, 1.
    if want("catalysts") then
        local catalysts = { "Abrasive", "Accelerating", "Dextral", "Fertile", "Imbued", "Intrinsic", "Noxious", "Prismatic", "Sinistral", "Tempering", "Turbulent", "Unstable" }
        for _, q in ipairs({ 0, 0.5, 1 }) do
            main.defaultItemAffixQuality = q
            for _, type_ in ipairs({ "amulet", "belt", "ring" }) do
                run("catalyst:" .. type_, function()
                    for _, raw in sortedPairs(data.uniques[type_] or {}) do
                        for _, c in ipairs(catalysts) do
                            local withCat = raw:gsub("^([^\n]*\n[^\n]*\n)", "%1Catalyst: " .. c .. "\nCatalystQuality: 20\n", 1)
                            pcall(function() new("Item"):Item(withCat, "UNIQUE", true) end)
                        end
                    end
                end)
            end
        end
        main.defaultItemAffixQuality = 0.5
    end
    if want("rares") then
        run("rare", function()
            for _, raw in sortedPairs(data.rares) do
                pcall(function()
                    local newItem = new("Item"):Item(raw, "RARE", true)
                    if newItem.base and newItem.crafted then
                        if newItem.base.implicit and #newItem.implicitModLines == 0 then
                            local implicitIndex = 1
                            for line in newItem.base.implicit:gmatch("[^\n]+") do
                                table.insert(newItem.implicitModLines, { line = line, modTags = newItem.base.implicitModTypes and newItem.base.implicitModTypes[implicitIndex] or { } })
                                implicitIndex = implicitIndex + 1
                            end
                        end
                        newItem:Craft()
                    end
                end)
            end
        end)
    end
    -- Affix lines of Data/Mod*.lua at roll ranges 0, 0.5 and 1
    -- (itemLib.applyRange, as Item:Craft / ItemsTab do).
    if want("affixes") then
        for cat, mods in sortedPairs(data.itemMods) do
            if cat ~= "Item" then -- union of other categories (Data.lua:686-692)
                run("affix:" .. cat, function() parseAffixTable(mods) end)
            end
        end
        run("affix:Master", function() parseAffixTable(data.masterMods) end)
        run("affix:Veiled", function() parseAffixTable(data.veiledMods) end)
        run("affix:Necropolis", function() parseAffixTable(data.necropolisMods) end)
        run("affix:Crucible", function() parseAffixTable(data.crucible) end)
        run("affix:BeastCraft", function() parseAffixTable(data.beastCraft) end)
    end
    if want("enchantments") then
        run("enchant", function() parseStringLists(data.enchantments) end)
    end
    -- Cluster jewel enchants (each line, and joined as clusterJewelSkills
    -- keys them, ModParser.lua:6597-6607), small passive stats, notables,
    -- keystones.
    if want("cluster") then
        run("cluster", function()
            for _, jewel in sortedPairs(data.clusterJewels.jewels) do
                for _, skill in sortedPairs(jewel.skills) do
                    parseLineList(skill.enchant, ident)
                    P(t_concat(skill.enchant, " "))
                    if skill.stats then parseLineList(skill.stats, ident) end
                end
            end
            for notable in sortedPairs(data.clusterJewels.notableSortOrder) do
                P("1 Added Passive Skill is " .. notable)
            end
            for _, keystone in ipairs(data.clusterJewels.keystones) do
                P("Adds " .. keystone)
            end
        end)
    end
    -- Item base implicits (Data/Bases/*.lua), at every roll position.
    if want("bases") then
        run("base", function()
            for _, base in sortedPairs(data.itemBases) do
                -- flask / tincture buff lines (Item.lua:1058-1073)
                for _, kind in ipairs({ "flask", "tincture" }) do
                    if type(base[kind]) == "table" and type(base[kind].buff) == "table" then
                        for _, line in ipairs(base[kind].buff) do P(line) end
                    end
                end
                if type(base.implicit) == "string" then
                    local list = {}
                    for l in base.implicit:gmatch("[^\n]+") do list[#list + 1] = l end
                    for _, r in ipairs(RANGES) do
                        parseLineList(list, function(l)
                            local ok, x = pcall(itemLib.applyRange, l, r)
                            return ok and x or l
                        end)
                    end
                end
            end
        end)
    end
    -- Timeless jewel additions (Data/TimelessJewelData/LegionPassives.lua),
    -- parsed by PassiveSpec.lua:2405-2440 after their rolls are filled in;
    -- here at every roll position.
    if want("legion") then
        run("legion", function()
            local legion = LoadModule("Data/TimelessJewelData/LegionPassives")
            for _, addition in sortedPairs(legion.additions) do
                local list = {}
                for _, sd in ipairs(addition.sd or {}) do
                    for l in sd:gmatch("[^\n]+") do list[#list + 1] = l end
                end
                for _, r in ipairs(RANGES) do
                    parseLineList(list, function(l)
                        local ok, x = pcall(itemLib.applyRange, l, r)
                        return ok and x or l
                    end)
                end
            end
        end)
    end
    -- Data/ModFoulbornMap.lua: unique name -> its Foulborn mutation lines, at
    -- every roll position. (PoB itself maps mutations by mod id through
    -- data.foulbornMap from ModFoulbornMap.jsonc, Modules/Data.lua:1480-1482,
    -- whose targets are data.itemMods.Foulborn, covered by "affixes"; the
    -- .lua is a Data/Mod*.lua file of stat lines, so it is parsed too.)
    if want("foulborn") then
        run("foulborn", function()
            for _, mutated in sortedPairs(LoadModule("Data/ModFoulbornMap")) do
                local list = {}
                for _, l in ipairs(mutated) do
                    if type(l) == "string" then
                        for s in l:gmatch("[^\n]+") do list[#list + 1] = s end
                    end
                end
                for _, r in ipairs(RANGES) do
                    parseLineList(list, function(l)
                        local ok, x = pcall(itemLib.applyRange, l, r)
                        return ok and x or l
                    end)
                end
            end
        end)
    end
    if want("pantheons") then
        run("pantheon", function()
            for _, god in sortedPairs(data.pantheons) do
                for _, soul in ipairs(god.souls) do
                    for _, m in ipairs(soul.mods) do P(m.line) end
                end
            end
        end)
    end

    modLib.parseMod = origParse
    print, ConPrintf = savedPrint, savedCon

    -- Output: one JSON object per line, sorted by line (bytewise).
    local keys = {}
    for k in next, recs do keys[#keys + 1] = k end
    t_sort(keys)
    local out = {}
    local nMods, nNil, nExtra, nErr = 0, 0, 0, 0
    for i, line in ipairs(keys) do
        local rec = recs[line]
        local srcs = {}
        for s in next, rec.src do srcs[#srcs + 1] = s end
        t_sort(srcs)
        for j, s in ipairs(srcs) do srcs[j] = jstr(s) end
        local passes = {}
        for j, p in ipairs(rec.passes) do
            passes[j] = '{"order":' .. tostring(p.order) .. ',"steps":[' .. t_concat(p.steps, ",") .. "]}"
        end
        local s = '{"line":' .. jstring(line)
        if rec.err then
            nErr = nErr + 1
            s = s .. ',"mods":null,"extra":null,"error":' .. jstring(rec.err)
        else
            s = s .. ',"mods":' .. rec.mods .. ',"extra":' .. (rec.extra == nil and "null" or jstring(rec.extra))
            if rec.mods == "null" then nNil = nNil + 1 else nMods = nMods + 1 end
            if rec.extra ~= nil then nExtra = nExtra + 1 end
        end
        out[i] = s .. ',"passes":[' .. t_concat(passes, ",") .. '],"sources":[' .. t_concat(srcs, ",") .. "]}"
    end
    -- every key of every pattern table must have a source line
    local unresolvedKeys = {}
    for _, name in ipairs(LISTS) do
        for k in next, L[name] do
            if prov(L[name], k) == nil then unresolvedKeys[#unresolvedKeys + 1] = name .. "[" .. tostring(k) .. "]" end
        end
    end
    t_sort(unresolvedKeys)
    local listSizes = {}
    for _, name in ipairs(LISTS) do
        local n = 0
        for _ in next, L[name] do n = n + 1 end
        listSizes[name] = n
    end
    local info = {
        records = #keys, withMods = nMods, nilMods = nNil, withExtra = nExtra, errors = nErr,
        fullTies = stats.ties, jewelFuncTies = stats.jewelTies, unresolvedSteps = stats.unresolved,
        unresolvedKeys = unresolvedKeys, counts = counts, runErrors = errors, listSizes = listSizes,
        trackedLines = lines.tracked, scanLine = lines.scan, parseModLine = lines.parseMod,
    }
    recs = {}
    return out, info
end

return PO
