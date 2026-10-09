-- pob-dump boot: PoB's data environment, loaded the way Modules/Main.lua
-- does it (GameVersions, Common, CalcFormat, Data), with PoB's own headless
-- SimpleGraphic stubs (_SimpleGraphic.def.lua, as HeadlessWrapper.lua uses).

-- Deterministic pairs(). LuaJIT seeds its string hash randomly per state,
-- so pairs() order differs between runs, and a few results of PoB's load
-- depend on it (e.g. ExplosiveTrap and ExplosiveTrapAltX share one statMap
-- table and Data.lua's per-skill pass is last-writer-wins on its
-- _grantedEffect / mod sources; clusterJewelInfoForNotable[*].jewelTypes is
-- filled in pairs() order). Iterating in sorted key order picks one of the
-- orders PoB itself could produce, every time.
do
    local raw_next, rawget, type, t_sort = next, rawget, type, table.sort
    local rank = { boolean = 1, number = 2, string = 3 }
    local function keyLess(a, b)
        local ta, tb = type(a), type(b)
        if ta ~= tb then return (rank[ta] or 9) < (rank[tb] or 9) end
        if ta == "number" or ta == "string" then return a < b end
        if ta == "boolean" then return (not a) and b end
        return false -- table/function keys: no meaningful order
    end
    function pairs(t)
        local keys, n = {}, 0
        for k in raw_next, t do n = n + 1; keys[n] = k end
        t_sort(keys, keyLess)
        local i = 0
        return function()
            while i < n do
                i = i + 1
                local k = keys[i]
                local v = rawget(t, k)
                if v ~= nil then return k, v end
            end
        end, t, nil
    end
end

dofile("_SimpleGraphic.def.lua")

launch = { devMode = false, installedMode = false, versionNumber = "?", versionBranch = "dump", versionPlatform = "dump" }

-- Run a file with its globals captured: returns (definedGlobals, returnValue).
-- Definitions are then copied into _G so the rest of PoB sees them as usual.
local function capture(file)
    local env = setmetatable({}, { __index = _G })
    local fn = assert(loadfile(file))
    setfenv(fn, env)
    local ret = fn()
    local defs = {}
    for k, v in pairs(env) do
        defs[k] = v
        rawset(_G, k, v)
    end
    return defs, ret
end

__dump_roots = {}
__dump_roots.GameVersions = capture("GameVersions.lua")
LoadModule("Modules/Common")
-- Global.lua needs Common's copyTable. Data.lua loads it again itself;
-- this capture only records what it defines.
__dump_roots.global = capture("Data/Global.lua")
LoadModule("Modules/CalcFormat")
LoadModule("Modules/Data")

-- Keys of `data` that come from Data/Misc.lua (merged into `data` by
-- Data.lua); the dumper groups them into misc.json.
__dump_misc_keys = {}
for k in pairs(LoadModule("Data/Misc")) do __dump_misc_keys[k] = true end
