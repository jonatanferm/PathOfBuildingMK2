//! Path of Building itself, booted headless in the embedded interpreter.
//!
//! Mirrors PoB's own `HeadlessWrapper.lua` (used by its test suite): define
//! the SimpleGraphic API stubs, run `Launch.lua`, one `OnInit` and one
//! `OnFrame`, then drive builds through `main:SetMode("BUILD", …)`. The
//! unique/rare item browser databases are not loaded at boot (the calc never
//! reads them); [`Pob::load_item_dbs`] loads them on demand.

use std::rc::Rc;

use ahash::AHashMap;

use crate::lua::host::{self, HostFs};
use crate::lua::{Interp, LuaError, Val};

pub struct Pob {
    pub interp: Interp,
}

/// One Lua return value as the UI consumes it: strings verbatim, anything
/// else as Lua's `tostring` would print it.
#[derive(Debug, Clone)]
pub enum Ret {
    Str(String),
    Other(String),
}

impl Ret {
    #[must_use]
    pub fn text(&self) -> &str {
        match self {
            Self::Str(s) | Self::Other(s) => s,
        }
    }
}

/// A booted Path of Building, whichever Lua runs it.
pub trait Engine {
    /// Run a Lua chunk (with `build` bound), returning its results.
    fn run(&mut self, code: &str) -> Result<Vec<Ret>, String>;
    /// Load a build from PoB XML and calculate it.
    fn load_build_xml(&mut self, xml: &str, name: &str) -> Result<(), String>;
    /// The player's (and minion's, `Minion`-prefixed) calc outputs.
    fn player_output(&mut self) -> Result<Outputs, String>;
}

/// Load `__pob_xml` as build `__pob_name`.
pub const LOAD_SCRIPT: &str = r#"
__mainObject__.promptMsg = nil
__mainObject__.main:SetMode("BUILD", false, __pob_name, __pob_xml)
runCallback("OnFrame")
if __mainObject__.promptMsg then
    error("build load failed: " .. tostring(__mainObject__.promptMsg))
end
build = __mainObject__.main.modes["BUILD"]
if build.itemsTab then build.itemsTab:UpdateSockets() end
"#;

/// The main calc env's player and minion output tables.
pub const OUTPUT_SCRIPT: &str = r"
local b = __mainObject__.main.modes['BUILD']
local env = b.calcsTab and (b.calcsTab.mainEnv or b.calcsTab.calcsEnv)
return env and env.player and env.player.output, env and env.minion and env.minion.output
";

impl Engine for Pob {
    fn run(&mut self, code: &str) -> Result<Vec<Ret>, String> {
        Ok(self
            .exec(code)?
            .iter()
            .map(|v| match v {
                Val::Str(s) => Ret::Str(s.to_str().into_owned()),
                other => Ret::Other(other.to_string()),
            })
            .collect())
    }

    fn load_build_xml(&mut self, xml: &str, name: &str) -> Result<(), String> {
        Pob::load_build_xml(self, xml, name)
    }

    fn player_output(&mut self) -> Result<Outputs, String> {
        Pob::player_output(self)
    }
}

/// Scalar outputs of a calc (`env.player.output` / `calcsTab.mainOutput`).
#[derive(Debug, Clone, Default)]
pub struct Outputs {
    pub numbers: AHashMap<String, f64>,
    pub flags: AHashMap<String, bool>,
    pub strings: AHashMap<String, String>,
}

impl Outputs {
    #[must_use]
    pub fn get(&self, key: &str) -> Option<f64> {
        self.numbers.get(key).copied()
    }
}

fn err(e: LuaError) -> String {
    e.0
}

impl Pob {
    /// Boot PoB with `fs` rooted at PoB's `src/` directory.
    pub fn boot(fs: Rc<dyn HostFs>) -> Result<Self, String> {
        let mut it = Interp::new();
        host::install(&mut it, fs.clone());
        let wrapper = fs
            .read("HeadlessWrapper.lua")
            .ok_or("HeadlessWrapper.lua not found")?;
        let wrapper = String::from_utf8_lossy(&wrapper).into_owned();
        // Everything before `dofile("Launch.lua")`: SimpleGraphic stubs,
        // callback plumbing and the `require` hack.
        let prefix = wrapper
            .find("dofile(\"Launch.lua\")")
            .map_or(wrapper.as_str(), |i| &wrapper[..i]);
        it.exec_chunk(prefix, "HeadlessWrapper.lua", Vec::new())
            .map_err(err)?;
        // `_SimpleGraphic.def.lua` defined inert stubs for these; restore the
        // real host implementations.
        host::install_natives(&mut it, fs);
        it.exec_chunk("dofile('Launch.lua')", "boot", Vec::new())
            .map_err(err)?;
        it.exec_chunk(BOOT_SCRIPT, "boot", Vec::new())
            .map_err(err)?;
        Ok(Self { interp: it })
    }

    /// Boot from a PoB checkout on disk (`<root>/src`).
    pub fn boot_from_dir(pob_root: &std::path::Path) -> Result<Self, String> {
        Self::boot(Rc::new(host::StdFs {
            root: pob_root.join("src"),
        }))
    }

    /// Run a Lua snippet (for tests / tooling).
    pub fn exec(&mut self, src: &str) -> Result<Vec<Val>, String> {
        self.interp.exec_chunk(src, "exec", Vec::new()).map_err(err)
    }

    /// Load a build from PoB XML and calculate it.
    pub fn load_build_xml(&mut self, xml: &str, name: &str) -> Result<(), String> {
        self.interp.set_global("__pob_xml", Val::str(xml));
        self.interp.set_global("__pob_name", Val::str(name));
        self.exec(LOAD_SCRIPT)?;
        Ok(())
    }

    /// The main calc env's player output (what PoB's sidebar and Calcs tab
    /// show), plus the summoned minion's output under a `Minion` prefix.
    pub fn player_output(&mut self) -> Result<Outputs, String> {
        let v = self.exec(OUTPUT_SCRIPT)?;
        let mut out = Outputs::default();
        for (i, prefix) in [(0usize, ""), (1, "Minion")] {
            let Some(Val::Table(t)) = v.get(i) else {
                continue;
            };
            for (k, v) in t.pairs() {
                let Val::Str(k) = k else { continue };
                let k = format!("{prefix}{}", k.to_str());
                match v {
                    Val::Num(n) => {
                        out.numbers.insert(k, n);
                    }
                    Val::Bool(b) => {
                        out.flags.insert(k, b);
                    }
                    Val::Str(s) => {
                        out.strings.insert(k, s.to_str().into_owned());
                    }
                    _ => {}
                }
            }
        }
        Ok(out)
    }

    /// Recalculate the loaded build (`calcsTab:BuildOutput()`).
    pub fn recalc(&mut self) -> Result<(), String> {
        self.exec("build.calcsTab:BuildOutput()").map(|_| ())
    }
}

/// Run after `Launch.lua`: PoB's `OnInit` and first `OnFrame` (minus the
/// item-database load), then the helpers MK2's UI queries use
/// (`__mk2_json`, `__mk2_ctl`, `__mk2_tooltip`, `__mk2_sets`, …). Shared by
/// every engine.
pub const BOOT_SCRIPT: &str = r#"
__mainObject__.continuousIntegrationMode = nil
runCallback("OnInit")
local main = __mainObject__.main
if main and main.onFrameFuncs then
    -- Kept for `__mk2_load_item_dbs` (the unique / rare browsers).
    __mk2_load_items = main.onFrameFuncs["LoadItems"]
    main.onFrameFuncs["LoadItems"] = nil
end

-- Build PoB's unique and rare item databases on first use.
function __mk2_load_item_dbs()
    local m = __mainObject__.main
    local guard = 0
    while __mk2_load_items and (m.uniqueDB.loading or m.rareDB.loading) and guard < 100000 do
        m.onFrameFuncs["LoadItems"] = __mk2_load_items
        __mk2_load_items()
        guard = guard + 1
    end
    m.onFrameFuncs["LoadItems"] = nil
    __mk2_load_items = nil
end
runCallback("OnFrame")
if __mainObject__.promptMsg then
    error("PoB startup failed: " .. tostring(__mainObject__.promptMsg))
end
build = __mainObject__.main.modes["BUILD"]

-- JSON encoder for UI queries: arrays for sequences, objects otherwise;
-- PoB colour codes are kept; functions / cycles become null.
function __mk2_json(v, depth, seen)
    depth = depth or 0
    seen = seen or {}
    local t = type(v)
    if t == "nil" then return "null"
    elseif t == "boolean" then return tostring(v)
    elseif t == "number" then
        if v ~= v or v == math.huge or v == -math.huge then return "null" end
        return string.format("%.14g", v)
    elseif t == "string" then
        return '"' .. v:gsub('[%c"\\]', function(c)
            if c == '"' then return '\\"' elseif c == '\\' then return '\\\\'
            elseif c == '\n' then return '\\n' elseif c == '\t' then return '\\t' end
            return string.format('\\u%04x', c:byte())
        end) .. '"'
    elseif t == "table" then
        if seen[v] or depth > 12 then return "null" end
        if next(v) == nil then return "[]" end
        seen[v] = true
        local parts = {}
        local n = #v
        local isArray = n > 0
        if isArray then
            for k in pairs(v) do
                if type(k) ~= "number" or k < 1 or k > n or k ~= math.floor(k) then isArray = false break end
            end
        end
        if isArray then
            for i = 1, n do parts[#parts + 1] = __mk2_json(v[i], depth + 1, seen) end
            seen[v] = nil
            return "[" .. table.concat(parts, ",") .. "]"
        end
        for k, x in pairs(v) do
            if type(x) ~= "function" then
                parts[#parts + 1] = __mk2_json(tostring(k)) .. ":" .. __mk2_json(x, depth + 1, seen)
            end
        end
        seen[v] = nil
        return "{" .. table.concat(parts, ",") .. "}"
    end
    return "null"
end

-- A PoB UI control as data: dropdown items/selection, edit buffer or
-- checkbox state, plus PoB's own shown/enabled rules.
function __mk2_ctl(name, c)
    local shown, enabled = true, true
    if c.shown ~= nil then shown = c:GetProperty("shown") and true or false end
    if c.enabled ~= nil then enabled = c:GetProperty("enabled") and true or false end
    local d = { n = name, shown = shown, en = enabled }
    if c.ListControl and c.list and c.GetRowValue then
        -- A list box (PoB's notable / item / gem lists): rows as PoB draws
        -- them, selection by index.
        d.k = "list"
        -- DB lists (notables, uniques) build lazily in Draw; build now.
        if c.listBuildFlag and c.ListBuilder then
            c.listBuildFlag = false
            local co = coroutine.create(c.ListBuilder)
            local guard = 0
            repeat
                local ok = coroutine.resume(co, c)
                guard = guard + 1
            until not ok or coroutine.status(co) == "dead" or guard > 100000
            c.listOutputRevision = build and build.outputRevision
        end
        d.items = {}
        for i, v in ipairs(c.list) do
            local ok, text = pcall(c.GetRowValue, c, 1, i, v)
            d.items[i] = ok and tostring(text or "") or tostring(v)
        end
        d.sel = c.selIndex or 0
        return d
    end
    -- Width only for edit boxes: some controls' width functions have side
    -- effects (the Party tab re-wraps its notes label to the width).
    if c.buf ~= nil and type(c.width) == "number" then d.w = c.width end
    if c.label ~= nil then
        local ok, label = pcall(c.GetProperty, c, "label")
        if ok and type(label) == "string" then d.label = label end
    end
    if c.list then
        d.k = "drop"
        d.items = {}
        for i, v in ipairs(c.list) do
            d.items[i] = type(v) == "table" and tostring(v.label or v.val or "") or tostring(v)
        end
        d.sel = c.selIndex or 1
    elseif c.buf ~= nil then
        d.k = "edit"
        d.buf = c.buf
        if c.lineHeight then d.ml = true end
        if type(c.prompt) == "string" then d.prompt = c.prompt end
    elseif c.CheckBoxControl then
        d.k = "check"
        d.state = c.state and true or false
    elseif c.SliderControl then
        d.k = "slider"
        d.val = c.val or 0
    elseif c.ButtonControl then
        d.k = "button"
    elseif c.LabelControl then
        d.k = "label"
    else
        d.k = "other"
    end
    return d
end

-- A PoB Tooltip as JSON lines ({s = size, t = text}; no t = separator),
-- minus hints about PoB's keyboard shortcuts.
function __mk2_tooltip(tt)
    local out = {}
    for _, l in ipairs(tt.lines) do
        local text = l.text
        local plain = text and text:gsub("%^x%x%x%x%x%x%x", ""):gsub("%^%d", "")
        if text == nil then
            if #out > 0 and out[#out].t ~= nil then out[#out + 1] = { s = l.size } end
        elseif not plain:match("^%s*Tip:") and not (plain == "" and text ~= "") then
            out[#out + 1] = { s = l.size, t = text }
        end
    end
    while #out > 0 and out[#out].t == nil do out[#out] = nil end
    return __mk2_json(out)
end

-- PoB's tree specs / item sets / skill sets / config sets: titles and the
-- active index of each.
function __mk2_sets()
    local function ordered(order, sets, active)
        local t = { items = {}, sel = 1 }
        for i, id in ipairs(order) do
            t.items[i] = sets[id] and sets[id].title or "Default"
            if id == active then t.sel = i end
        end
        return t
    end
    return {
        spec = { items = build.treeTab:GetSpecList(), sel = build.treeTab.activeSpec or 1 },
        item = ordered(build.itemsTab.itemSetOrderList, build.itemsTab.itemSets, build.itemsTab.activeItemSetId),
        skill = ordered(build.skillsTab.skillSetOrderList, build.skillsTab.skillSets, build.skillsTab.activeSkillSetId),
        config = ordered(build.configTab.configSetOrderList, build.configTab.configSets, build.configTab.activeConfigSetId),
    }
end

-- Select / new / copy / rename / delete a set, through PoB's own manage
-- list control and its name / confirm popup.
function __mk2_set_op(kind, op, index, title)
    if op == "select" then
        if kind == "spec" then
            build.treeTab:SetActiveSpec(index)
        elseif kind == "item" then
            build.itemsTab:SetActiveItemSet(build.itemsTab.itemSetOrderList[index])
        elseif kind == "skill" then
            build.skillsTab:SetActiveSkillSet(build.skillsTab.skillSetOrderList[index])
        else
            build.configTab:SetActiveConfigSet(build.configTab.configSetOrderList[index])
        end
        return
    end
    local kinds = {
        spec = { "PassiveSpecListControl", build.treeTab },
        item = { "ItemSetListControl", build.itemsTab },
        skill = { "SkillSetListControl", build.skillsTab },
        config = { "ConfigSetListControl", build.configTab },
    }
    local k = kinds[kind]
    local ctl = new(k[1])
    ctl[k[1]](ctl, nil, { 0, 0, 300, 300 }, k[2])
    if index then
        ctl.selIndex = index
        ctl.selValue = ctl.list[index]
    end
    ctl.controls[op].onClick()
    local p = main.popups[1]
    if p then
        if title and p.controls.edit then p.controls.edit:SetText(title, true) end
        local b = p.controls.save or p.controls.confirm
        if b then b.onClick() end
    end
    while main.popups[1] do main:ClosePopup() end
end

-- The Skills tab: socket groups, and PoB's own controls for the group
-- being edited (`skillsTab.displayGroup`).
function __mk2_skills()
    local st = build.skillsTab
    local out = { groups = {}, display = 0, main = build.mainSocketGroup }
    for i, g in ipairs(st.socketGroupList) do
        out.groups[i] = {
            label = g.displayLabel or g.label or "",
            enabled = g.enabled and true or false,
            slot = g.slot or "",
            full = g.includeInFullDPS and true or false,
            source = g.source and true or false,
            n = #g.gemList,
        }
        if g == st.displayGroup then out.display = i end
    end
    local g = st.displayGroup
    if g then
        st:SetDisplayGroup(g)
        local c = st.controls
        out.ctls = {
            __mk2_ctl("groupLabel", c.groupLabel),
            __mk2_ctl("groupSlot", c.groupSlot),
            __mk2_ctl("groupEnabled", c.groupEnabled),
            __mk2_ctl("includeInFullDPS", c.includeInFullDPS),
            __mk2_ctl("groupCount", c.groupCount),
        }
        out.fromItem = g.source and true or false
        out.gems = {}
        for i = 1, #g.gemList + 1 do
            local s = st.gemSlots[i]
            local gi = g.gemList[i]
            if s then
                out.gems[i] = {
                    name = s.nameSpec.buf or "",
                    color = gi and gi.color or "",
                    err = gi and gi.errMsg or "",
                    level = __mk2_ctl("level", s.level),
                    quality = __mk2_ctl("quality", s.quality),
                    enabled = __mk2_ctl("enabled", s.enabled),
                    count = __mk2_ctl("count", s.count),
                    g1 = __mk2_ctl("enableGlobal1", s.enableGlobal1),
                    g2 = __mk2_ctl("enableGlobal2", s.enableGlobal2),
                }
            end
        end
    end
    return __mk2_json(out)
end

-- Every gem PoB offers: name, support flag, colour (1 str, 2 dex, 3 int).
function __mk2_gem_catalog()
    local out = {}
    for _, g in pairs(build.data.gems) do
        if not g.grantedEffect.hideFromGemList then
            out[#out + 1] = {
                n = g.name,
                s = g.grantedEffect.support and true or false,
                c = g.grantedEffect.color or 0,
                l = g.grantedEffect.legacy and true or false,
            }
        end
    end
    table.sort(out, function(a, b) return a.n < b.n end)
    return __mk2_json(out)
end

local function gemIdByName(name)
    for id, g in pairs(build.data.gems) do
        if g.name == name then return id end
    end
end

-- Skills tab edits, through PoB's own group list and gem slot controls.
function __mk2_skills_op(op, a, b, c)
    local st = build.skillsTab
    local list = st.controls.groupList
    if op == "select" then
        local g = st.socketGroupList[a]
        list.selIndex, list.selValue = a, g
        st:SetDisplayGroup(g)
    elseif op == "new" then
        list.controls.new.onClick()
    elseif op == "delete" then
        local g = st.socketGroupList[a]
        if g then
            list.selIndex, list.selValue = a, g
            list:OnSelDelete(a, g)
            local p = main.popups[1]
            if p and p.controls.confirm then p.controls.confirm.onClick() end
            while main.popups[1] do main:ClosePopup() end
        end
    elseif op == "gem" then
        local s, id = st.gemSlots[a], gemIdByName(b)
        if s and id then
            s.nameSpec:SetText(b)
            s.nameSpec.gemChangeFunc(id, true, false, true)
        end
    elseif op == "gemdel" then
        local s = st.gemSlots[a]
        if s then s.delete.onClick() end
    elseif op == "gemctl" then
        local s = st.gemSlots[a]
        if s and s[b] then __mk2_set_ctl(s[b], c) end
    elseif op == "ctl" then
        __mk2_set_ctl(st.controls[a], b)
    elseif op == "main" then
        build.mainSocketGroup = a
    end
end

-- PoB's tooltip for gem `i` of the edited group, or for socket group `g`.
function __mk2_gem_tooltip(i)
    local g = build.skillsTab.displayGroup
    local gi = g and g.gemList[i]
    if not gi then return "[]" end
    local tt = new("Tooltip"):Tooltip()
    require("Classes.GemTooltip").AddGemTooltip(tt, build, gi)
    return __mk2_tooltip(tt)
end
function __mk2_group_tooltip(i)
    local g = build.skillsTab.socketGroupList[i]
    if not g then return "[]" end
    local tt = new("Tooltip"):Tooltip()
    build.skillsTab:AddSocketGroupTooltip(tt, g)
    return __mk2_tooltip(tt)
end

-- The Items tab's slots as PoB shows them: base slots, the active weapon
-- set, flasks (with activation), abyssal and tree jewel sockets.
function __mk2_item_slots()
    local it = build.itemsTab
    it:UpdateSockets()
    it:PopulateSlots()
    local out = { swap = it.activeItemSet.useSecondWeaponSet and true or false, slots = {} }
    for _, slot in ipairs(it.orderedSlots) do
        local d = __mk2_ctl(slot.slotName, slot)
        if d.shown then
            d.label = slot.label
            local item = it.items[slot.selItemId]
            d.item = item and item.name or ""
            d.rarity = item and item.rarity or ""
            d.abyssal = slot.parentSlot and true or false
            d.jewel = slot.nodeId and true or false
            if slot.controls.activate then
                d.flask = true
                d.active = slot.active and true or false
            end
            out.slots[#out.slots + 1] = d
        end
    end
    return __mk2_json(out)
end

function __mk2_item_slot_op(op, name, v)
    local it = build.itemsTab
    if op == "sel" then
        __mk2_set_ctl(it.slots[name], v)
    elseif op == "activate" then
        __mk2_set_ctl(it.slots[name].controls.activate, v)
    elseif op == "swap" then
        local b = v and it.controls.weaponSwap2 or it.controls.weaponSwap1
        b.onClick()
    end
end

-- PoB's tooltip for whatever is in slot `name`.
function __mk2_slot_tooltip(name)
    local it = build.itemsTab
    local slot = it.slots[name]
    local item = slot and it.items[slot.selItemId]
    if not item then return "[]" end
    local tt = new("Tooltip"):Tooltip()
    it:AddItemTooltip(tt, item, slot)
    return __mk2_tooltip(tt)
end

-- Network: with a host HTTP function (desktop), serve launch:DownloadPage
-- synchronously but run its callback after the current command, as PoB
-- expects it to arrive later.
__mk2_callbacks = {}
if __mk2_http then
    function launch:DownloadPage(url, callback, params)
        params = params or {}
        local body, err, header = __mk2_http(url, params.header, params.body, "Path of Building/" .. tostring(self.versionNumber))
        __mk2_callbacks[#__mk2_callbacks + 1] = function()
            callback({ header = header or "", body = body or "" }, err)
        end
    end
end
function __mk2_flush_callbacks()
    local n = 0
    while #__mk2_callbacks > 0 and n < 50 do
        local cb = table.remove(__mk2_callbacks, 1)
        local ok, e = pcall(cb)
        if not ok then ConPrintf("callback error: %s", tostring(e)) end
        n = n + 1
    end
end

-- Drive a PoB control the way a click / edit would.
function __mk2_set_ctl(c, v)
    if c.ListControl and c.list then
        local i = tonumber(v)
        c.selIndex, c.selValue = i, c.list[i]
        if c.OnSelect then c:OnSelect(i, c.list[i]) end
    elseif c.list then
        c:SetSel(tonumber(v))
    elseif c.buf ~= nil then
        c:SetText(tostring(v), true)
    elseif c.CheckBoxControl then
        c.state = v and true or false
        if c.changeFunc then c.changeFunc(c.state) end
    elseif c.SliderControl then
        c:SetVal(tonumber(v) or 0)
    elseif c.ButtonControl then
        local before = main.popups[1]
        if c.onClick then c.onClick() end
        -- A button that asks "are you sure?" was clicked on purpose: accept.
        local p = main.popups[1]
        if p and p ~= before and p.controls.confirm and not p.controls.save then
            p.controls.confirm.onClick()
            if main.popups[1] == p then main:ClosePopup() end
        end
    end
end

-- Visible controls of a control table, in reading order (top to bottom,
-- left to right, by PoB's own layout). `keep(name)` filters by name.
function __mk2_ctls(tbl, keep)
    local list = {}
    for name, c in pairs(tbl) do
        if type(name) == "string" and type(c) == "table" and c.GetProperty and (not keep or keep(name)) then
            local ok, d = pcall(__mk2_ctl, name, c)
            local okShown, shown = pcall(function() return c:IsShown() end)
            if ok and d.shown and (not okShown or shown) and d.k ~= "other" then
                local okp, x, y = pcall(c.GetPos, c)
                d.x, d.y = okp and x or 0, okp and y or 0
                list[#list + 1] = d
            end
        end
    end
    table.sort(list, function(a, b)
        if math.abs(a.y - b.y) > 4 then return a.y < b.y end
        return a.x < b.x
    end)
    return list
end

-- The topmost PoB popup (title + controls), or nil.
function __mk2_popup()
    local p = main.popups[1]
    if not p then return nil end
    return { title = p.title or "", ctls = __mk2_ctls(p.controls) }
end

-- The Items tab's item editor: PoB's display-item controls, the item's
-- tooltip and any open popup (craft / create / edit / enchant / ...).
function __mk2_item_editor()
    local it = build.itemsTab
    local keepNames = { craftDisplayItem = true, newDisplayItem = true, addDisplayItem = true,
        editDisplayItem = true, removeDisplayItem = true, craftingSorting = true, craftingSortingLabel = true,
        priceDisplayItem = true }
    local out = {
        has = it.displayItem ~= nil,
        ctls = __mk2_ctls(it.controls, function(n) return keepNames[n] or n:match("^displayItem") ~= nil end),
        popup = __mk2_popup(),
    }
    if it.displayItem then
        local tt = new("Tooltip"):Tooltip()
        it:AddItemTooltip(tt, it.displayItem, nil)
        out.tooltip = __mk2_tooltip(tt)
    end
    return __mk2_json(out)
end

-- Put the item in PoB slot `slotName` into the editor (a copy, as PoB's
-- item list does on click).
function __mk2_edit_slot_item(slotName)
    local it = build.itemsTab
    local slot = it.slots[slotName]
    local item = slot and it.items[slot.selItemId]
    if item then
        -- As PoB's item list does on double-click: a fresh copy that keeps
        -- the id, so "Save" replaces the original.
        local newItem = new("Item"):Item(item:BuildRaw())
        newItem.id = item.id
        it:SetDisplayItem(newItem)
    end
end
"#;
