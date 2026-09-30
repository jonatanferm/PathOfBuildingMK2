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
        it.exec_chunk(
            r#"
__mainObject__.continuousIntegrationMode = nil
runCallback("OnInit")
local main = __mainObject__.main
if main and main.onFrameFuncs then
    main.onFrameFuncs["LoadItems"] = nil
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
    local shown = c:GetProperty("shown")
    if shown == nil then shown = true end
    local enabled = c:GetProperty("enabled")
    if enabled == nil then enabled = true end
    local d = { n = name, shown = shown and true or false, en = enabled and true or false }
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
    elseif c.changeFunc and c.state ~= nil or c.CheckBoxControl then
        d.k = "check"
        d.state = c.state and true or false
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

-- Drive a PoB control the way a click / edit would.
function __mk2_set_ctl(c, v)
    if c.list then
        c:SetSel(tonumber(v))
    elseif c.buf ~= nil then
        c:SetText(tostring(v), true)
    elseif c.CheckBoxControl then
        c.state = v and true or false
        if c.changeFunc then c.changeFunc(c.state) end
    end
end
"#,
            "boot",
            Vec::new(),
        )
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
        self.exec(
            r#"
__mainObject__.promptMsg = nil
__mainObject__.main:SetMode("BUILD", false, __pob_name, __pob_xml)
runCallback("OnFrame")
if __mainObject__.promptMsg then
    error("build load failed: " .. tostring(__mainObject__.promptMsg))
end
build = __mainObject__.main.modes["BUILD"]
"#,
        )?;
        Ok(())
    }

    /// The main calc env's player output (what PoB's sidebar and Calcs tab
    /// show), plus the summoned minion's output under a `Minion` prefix.
    pub fn player_output(&mut self) -> Result<Outputs, String> {
        let v = self.exec(
            r"
local b = __mainObject__.main.modes['BUILD']
local env = b.calcsTab and (b.calcsTab.mainEnv or b.calcsTab.calcsEnv)
return env and env.player and env.player.output, env and env.minion and env.minion.output
",
        )?;
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
