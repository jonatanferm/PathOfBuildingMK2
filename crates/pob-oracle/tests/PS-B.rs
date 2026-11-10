//! Unit PS-B acceptance (calc doc §11.1, row PS-B): `PassiveSpec:BuildAllDependsAndPaths`
//! (`pob_calc::classes::passive_spec_paths`, Classes/PassiveSpec.lua 1092-1695) against PoB's
//! own under LuaJIT (interpreter, `JitPob::boot_oracle`), on every `<Spec>` of the corpus
//! (`.parity/builds`, with the build's real jewels) and on 62 synthetic timeless / abyss /
//! intuitive-leap builds (a corpus spec with one stand-in jewel in an allocated socket).
//!
//! PS-C (`ReplaceNode`, `NodeAdditionOrReplacementFromString`, `ReconnectNodeToClassStart`,
//! `NodeInKeystoneRadius`, `BuildClusterJewelGraphs`; it has its own test) is replaced on both
//! sides by the same stand-ins: PS-B's calls into PS-C are recorded (the port through
//! `passive_spec_paths::set_psc_calls`, the oracle by replacing the methods) and compared in
//! order, together with PS-B's `ConPrintf` lines. The `ReplaceNode` stand-in copies only the
//! fields PS-B reads back (`sd`, `dn`, `name`, `isTattoo`, `overrideType`, with Lua's nil
//! fall-through to the tree node), `NodeInKeystoneRadius` is PoB's own on the oracle side
//! and a test copy on the port side, the others only record. Outside `BuildAllDependsAndPaths`
//! the oracle's `ReplaceNode` and `BuildClusterJewelGraphs` are no-ops, and so are the port's
//! (`passive_spec::set_spec_stubs`, CCR-0052 §12).
//!
//! Orders: PoB's `pairs(spec.nodes)` and `nodesInRadius` orders are random per process for
//! 3.10+ trees (CCR-0024 §5). Both sides therefore rebuild `spec.nodes` (right after the
//! constructor) and every tree node's `nodesInRadius[r]` with the keys inserted in ascending
//! order; LuaJIT's layout of such a table is deterministic, so every order PS-B produces
//! (`depends`, `path`, the call trace, `allocNodes`) is compared exactly. One order stays
//! random in PoB: `pairs(changedNode.stats)` of an abyss legion node (string keys,
//! PassiveSpec.lua:1261). When two of its stats share a `(min-max)` text the roll of whichever
//! comes first replaces both lines, so a trace that differs only in the numbers of such
//! `NodeAdditionOrReplacementFromString` lines of abyss-conquered nodes is reported and
//! tolerated (CCR-0070 §4), not failed.
//!
//! Per spec: `Load` (its `ImportFromNodeList` runs PS-B), [synthetic jewel + PS-B], PS-B again,
//! three `DeallocNode` and three `AllocNode` (each runs PS-B). After every step: allocNodes
//! (order), masterySelections, the mastery / notable / keystone / tattoo counters,
//! splitPersonalityPath, ignoredNodes, checkNodeLinks, the PS-C call trace, and for every
//! node: alloc, depends, intuitiveLeapLikesAffecting, conqueredBy (seed, conqueror, abyss
//! modification), pathDist, path, distanceToClassStart, connectedToStart, visited,
//! allMasteryOptions and (masteries and replaced nodes) sd / reminderText / dn.
//!
//! Self-skips without a PoB checkout, `data/dump` or the corpus (`POB_ORACLE_REQUIRE=1`
//! makes that a failure). `PSB_BUILDS=substr,...` limits the corpus builds, `PSB_SYNTH=0`
//! skips the synthetic builds.
#![allow(non_snake_case, clippy::too_many_lines)]

use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::Arc;

use pob_calc::classes::passive_spec::{main_LoadTree, new_PassiveSpec, set_spec_stubs, SpecStubs};
use pob_calc::classes::passive_spec_paths::{set_psc_calls, PscCalls};
use pob_calc::classes::passive_tree::{node_id_val, nodesInRadiusAt};
use pob_calc::classes::{Item, Node, NodeRef, PassiveSpec, PassiveSpecRef, PassiveTreeRef};
use pob_calc::lua_table::{Key, LStr, Table, Val};
use pob_calc::model::{Build, BuildRef, XmlChild, XmlNode};
use pob_gamedata::timeless_lut as lut;
use pob_gamedata::{data, Name, Value, ValueTable};
use pob_lua::{tostring_num, Field, LjIntMap};
use pob_oracle::headless::{Engine, Ret};
use pob_oracle::jit::JitPob;
use pob_oracle::oracle::{find_parity_dir, find_pob_src, find_up};

fn skip(why: &str) {
    let required = std::env::var_os("POB_ORACLE_REQUIRE").is_some_and(|v| v == "1")
        || std::env::var_os("CI").is_some_and(|v| !v.is_empty() && v != "false" && v != "0");
    assert!(!required, "required input missing: {why}");
    eprintln!("SKIPPED: {why}");
}

fn init_data() -> bool {
    if pob_gamedata::is_initialised() {
        return true;
    }
    let dir = std::env::var_os("POB_DUMP_DIR")
        .map(PathBuf::from)
        .or_else(|| find_up("data/dump/data.bin").map(|p| p.parent().unwrap().to_path_buf()));
    let Some(dir) = dir else { return false };
    if !dir.join("data.bin").is_file() {
        return false;
    }
    pob_gamedata::init_from_dir(&dir).unwrap_or_else(|e| panic!("data.bin: {e}"));
    true
}

/// Lua run before PoB loads: refuse writes into the jewel data folder (the `.bin` caches).
const PRE: &str = r#"
do
    local open = io.open
    io.open = function(path, mode)
        if mode and mode:find("w") and tostring(path):find("TimelessJewelData") then
            return nil, tostring(path) .. ": read-only (PS-B test)"
        end
        return open(path, mode)
    end
end
"#;

const LIB: &str = r##"
if launch then launch.ShowErrMsg = function() end end
if not common.classes.PassiveSpec then LoadModule("Classes/PassiveSpec") end
__psb = { rec = 0, mute = 0, trace = { } }
ConPrintf = function(fmt)
    if __psb.rec > 0 and __psb.mute == 0 then __psb.trace[#__psb.trace + 1] = "P\t" .. tostring(fmt) end
end
-- the LUT readers print their file loading (cache dependent): not PS-B's lines
for _, f in ipairs({ "readLUT", "readAbyssJewelLUT" }) do
    local orig = data[f]
    data[f] = function(...)
        __psb.mute = __psb.mute + 1
        local r = orig(...)
        __psb.mute = __psb.mute - 1
        return r
    end
end
local PSC = common.classes.PassiveSpec
local origBADP = PSC.BuildAllDependsAndPaths
PSC.BuildAllDependsAndPaths = function(self)
    __psb.rec = __psb.rec + 1
    origBADP(self)
    __psb.rec = __psb.rec - 1
end
PSC.BuildClusterJewelGraphs = function() end
local function rec(s) __psb.trace[#__psb.trace + 1] = s end
-- `old.x = new.x`: nil falls through to the tree node
PSC.ReplaceNode = function(self, old, newNode)
    if __psb.rec == 0 then return end
    local sd = newNode.sd
    rec("R\t" .. tostring(old.id) .. "\t" .. tostring(newNode.id) .. "\t" .. tostring(newNode.dn))
    if old.sd == sd then return 1 end
    old.dn = newNode.dn
    old.sd = newNode.sd
    old.name = newNode.name
    old.isTattoo = newNode.isTattoo
    old.overrideType = newNode.overrideType
end
PSC.NodeAdditionOrReplacementFromString = function(self, node, sd, replacement)
    rec("A\t" .. tostring(node.id) .. "\t" .. sd .. "\t" .. tostring(replacement == true))
end
PSC.ReconnectNodeToClassStart = function(self, node)
    rec("C\t" .. tostring(node.id))
end

local function esc(s) return (tostring(s):gsub("\\", "\\\\"):gsub("\t", "\\t"):gsub("\n", "\\n")) end
local function ser(node, out)
    out[#out + 1] = "E\t" .. esc(node.elem)
    local ks = { }
    for k in pairs(node.attrib or { }) do ks[#ks + 1] = k end
    table.sort(ks)
    for _, k in ipairs(ks) do out[#out + 1] = "A\t" .. esc(k) .. "\t" .. esc(node.attrib[k]) end
    for _, c in ipairs(node) do
        if type(c) == "table" then ser(c, out) else out[#out + 1] = "T\t" .. esc(c) end
    end
    out[#out + 1] = "/"
end
local function sortedKeys(t) local p = { } for k in pairs(t) do p[#p + 1] = k end table.sort(p) return p end
local function joinNums(list) local p = { } for i, v in ipairs(list) do p[i] = tostring(v) end return table.concat(p, ",") end
local function keysInOrder(t) local p = { } for k in pairs(t) do p[#p + 1] = tostring(k) end return table.concat(p, ",") end
local function kvOrder(t) local p = { } for k, v in pairs(t) do p[#p + 1] = tostring(k) .. "=" .. tostring(v) end return table.concat(p, ",") end
local function kvSorted(t) local p = { } for _, k in ipairs(sortedKeys(t)) do p[#p + 1] = tostring(k) .. "=" .. tostring(t[k]) end return table.concat(p, ",") end
local function sdl(sd) return table.concat(sd or { }, "/") end

-- a build's specs and its items (as ItemsTab:Load builds them)
function __psb.load(xmlText)
    local dbXML = common.xml.ParseXML(xmlText)
    __psb.specNodes = { }
    __psb.items = { }
    local out, items = { }, { }
    for _, top in ipairs(dbXML) do
        if type(top) == "table" and top.elem == "PathOfBuilding" then
            for _, sec in ipairs(top) do
                if type(sec) == "table" and sec.elem == "Tree" then
                    for _, sp in ipairs(sec) do
                        if type(sp) == "table" and sp.elem == "Spec" then
                            __psb.specNodes[#__psb.specNodes + 1] = sp
                            ser(sp, out)
                        end
                    end
                elseif type(sec) == "table" and sec.elem == "Items" then
                    for _, node in ipairs(sec) do
                        if type(node) == "table" and node.elem == "Item" then
                            local item = new("Item"):Item("")
                            item.id = tonumber(node.attrib.id)
                            item.variant = tonumber(node.attrib.variant)
                            for _, a in ipairs({ "variantAlt", "variantAlt2", "variantAlt3", "variantAlt4", "variantAlt5" }) do
                                if node.attrib[a] then item[a] = tonumber(node.attrib[a]) end
                            end
                            for _, child in ipairs(node) do
                                if type(child) == "string" then item:ParseRaw(child) end
                            end
                            if item.base and item.id then
                                item:BuildModList()
                                __psb.items[item.id] = item
                            end
                        end
                    end
                end
            end
        end
    end
    -- the jewel fields PS-B (and the PS-A searches it calls) read
    for _, id in ipairs(sortedKeys(__psb.items)) do
        local item = __psb.items[id]
        local jd = item.jewelData
        items[#items + 1] = "I\t" .. id .. "\t" .. tostring(item.jewelRadiusIndex) .. "\t" .. tostring(jd ~= nil)
        if jd then
            for _, k in ipairs(sortedKeys(jd)) do
                local v = jd[k]
                local t = type(v)
                if k == "conqueredBy" then
                    items[#items + 1] = "C\t" .. id .. "\t" .. tostring(v.id) .. "\t" .. esc(v.conqueror.type) .. "\t" .. type(v.conqueror.id) .. "\t" .. esc(v.conqueror.id)
                elseif k == "impossibleEscapeKeystones" then
                    items[#items + 1] = "K\t" .. id .. "\t" .. esc(table.concat(sortedKeys(v), "\1"))
                elseif t == "boolean" or t == "number" or t == "string" then
                    items[#items + 1] = "J\t" .. id .. "\t" .. esc(k) .. "\t" .. t .. "\t" .. esc(v)
                end
            end
        end
    end
    return table.concat(out, "\n"), table.concat(items, "\n")
end

local function ids(list)
    if not list then return "nil" end
    local p = { }
    for i, n in ipairs(list) do p[i] = tostring(n.id) end
    return table.concat(p, ",")
end
local function conq(c)
    if not c then return "nil" end
    local parts = { tostring(c.id), tostring(c.conqueror and c.conqueror.type), tostring(c.conqueror and c.conqueror.id) }
    if c.modification then
        for _, comp in ipairs(c.modification) do
            local r = { }
            for i, x in ipairs(comp.rolls or { }) do r[i] = tostring(x) end
            parts[#parts + 1] = tostring(comp.type) .. "/" .. tostring(comp.id) .. "/" .. table.concat(r, "/")
        end
    end
    return table.concat(parts, ":")
end
local function state(spec, step, emit)
    emit(step .. "/alloc", keysInOrder(spec.allocNodes))
    emit(step .. "/mastery", kvOrder(spec.masterySelections))
    emit(step .. "/counts", table.concat({ tostring(spec.allocatedMasteryCount), tostring(spec.allocatedNotableCount),
        tostring(spec.allocatedKeystoneCount), tostring(spec.allocatedMasteryTypeCount),
        kvSorted(spec.allocatedMasteryTypes or { }), kvSorted(spec.allocatedTattooTypes or { }) }, "|"))
    emit(step .. "/split", joinNums(sortedKeys(spec.splitPersonalityPath or { })))
    emit(step .. "/ignored", joinNums(sortedKeys(spec.ignoredNodes or { })))
    emit(step .. "/cnl", tostring(spec.checkNodeLinks == true))
    emit(step .. "/trace", table.concat(__psb.trace, "\n"))
    __psb.trace = { }
    for _, id in ipairs(sortedKeys(spec.nodes)) do
        local n = spec.nodes[id]
        local t = spec.tree.nodes[id]
        local parts = { tostring(n.alloc == true), ids(n.depends), ids(n.intuitiveLeapLikesAffecting), conq(n.conqueredBy),
            tostring(n.pathDist), ids(n.path), tostring(n.distanceToClassStart), tostring(n.connectedToStart == true),
            tostring(n.visited == true), tostring(n.allMasteryOptions == true) }
        if n.type == "Mastery" or not t or n.sd ~= t.sd then
            parts[#parts + 1] = sdl(n.sd) .. "#" .. sdl(n.reminderText) .. "#" .. tostring(n.dn)
        end
        emit(step .. "/n/" .. id, table.concat(parts, "|"))
    end
end

local build = { itemsTab = { items = { } }, treeTab = { } }
local renormed = { }
local function renormTree(tree)
    if renormed[tree] then return end
    renormed[tree] = true
    for _, node in pairs(tree.nodes) do
        if node.nodesInRadius then
            for r, m in pairs(node.nodesInRadius) do
                local nm = { }
                for _, k in ipairs(sortedKeys(m)) do nm[k] = m[k] end
                node.nodesInRadius[r] = nm
            end
        end
    end
end
local function newSpec(ver)
    renormTree(main:LoadTree(ver))
    local spec = new("PassiveSpec"):PassiveSpec(build, ver)
    local nt = { }
    for _, k in ipairs(sortedKeys(spec.nodes)) do nt[k] = spec.nodes[k] end
    spec.nodes = nt
    return spec
end

function __psb.run(i, synth)
    build.itemsTab.items = { }
    for k, v in pairs(__psb.items) do build.itemsTab.items[k] = v end
    __psb.trace = { }
    local out = { }
    local function emit(k, v) out[#out + 1] = k .. "\t" .. esc(v) end
    local node = __psb.specNodes[i]
    local ver = node.attrib.treeVersion or defaultTreeVersion
    local spec = newSpec(ver)
    __psb.trace = { }
    spec:Load(node, "corpus")
    state(spec, "load", emit)
    if synth then
        build.itemsTab.items[synth.itemId] = { id = synth.itemId, jewelRadiusIndex = synth.radius, jewelData = synth.jd }
        spec.jewels[synth.socket] = synth.itemId
        spec:BuildAllDependsAndPaths()
        state(spec, "synth", emit)
    end
    spec:BuildAllDependsAndPaths()
    state(spec, "again", emit)
    local ops = { }
    local cand = { }
    for _, id in ipairs(sortedKeys(spec.allocNodes)) do
        local n = spec.allocNodes[id]
        if n.type ~= "ClassStart" and n.type ~= "AscendClassStart" then cand[#cand + 1] = id end
    end
    local picks = { }
    for k = 1, 3 do
        local j = math.floor(k * #cand / 4)
        if j >= 1 and cand[j] and picks[#picks] ~= cand[j] then picks[#picks + 1] = cand[j] end
    end
    for k, id in ipairs(picks) do
        if spec.allocNodes[id] then
            ops[#ops + 1] = "D" .. k .. ":" .. id
            spec:DeallocNode(spec.nodes[id])
            state(spec, "dealloc" .. k, emit)
        end
    end
    cand = { }
    for _, id in ipairs(sortedKeys(spec.nodes)) do
        local n = spec.nodes[id]
        if not n.alloc and n.path and n.pathDist >= 1 and n.pathDist <= 4 then cand[#cand + 1] = id end
    end
    picks = { }
    for k = 1, 3 do
        local j = math.floor(k * #cand / 4)
        if j >= 1 and cand[j] and picks[#picks] ~= cand[j] then picks[#picks + 1] = cand[j] end
    end
    for k, id in ipairs(picks) do
        ops[#ops + 1] = "A" .. k .. ":" .. id
        spec:AllocNode(spec.nodes[id])
        state(spec, "alloc" .. k, emit)
    end
    emit("ops", table.concat(ops, ","))
    return table.concat(out, "\n")
end

function __psb.clearLUTs()
    for t = 1, 11 do data.timelessJewelLUTs[t] = nil end
    collectgarbage("collect")
end
"##;

// ---- records -------------------------------------------------------------------------------

fn esc(s: &str) -> String {
    s.replace('\\', "\\\\")
        .replace('\t', "\\t")
        .replace('\n', "\\n")
}

fn unesc(s: &str) -> String {
    let mut out = String::new();
    let mut it = s.chars();
    while let Some(c) = it.next() {
        if c == '\\' {
            match it.next() {
                Some('t') => out.push('\t'),
                Some('n') => out.push('\n'),
                Some(o) => out.push(o),
                None => {}
            }
        } else {
            out.push(c);
        }
    }
    out
}

fn parse_records(s: &str) -> BTreeMap<String, String> {
    let mut out = BTreeMap::new();
    for line in s.split('\n') {
        if line.is_empty() {
            continue;
        }
        let (k, v) = line.split_once('\t').unwrap_or((line, ""));
        out.insert(k.to_owned(), v.to_owned());
    }
    out
}

struct Rec(BTreeMap<String, String>);
impl Rec {
    fn emit(&mut self, k: impl Into<String>, v: impl AsRef<str>) {
        self.0.insert(k.into(), esc(v.as_ref()));
    }
}

/// The serialised XML of `__psb.load` → nodes.
fn parse_xml(s: &str) -> Vec<XmlNode> {
    let mut stack: Vec<XmlNode> = Vec::new();
    let mut top: Vec<XmlNode> = Vec::new();
    for line in s.split('\n') {
        if line.is_empty() {
            continue;
        }
        let mut parts = line.splitn(3, '\t');
        match parts.next().unwrap() {
            "E" => stack.push(XmlNode {
                elem: LStr::from(unesc(parts.next().unwrap()).as_str()),
                ..XmlNode::default()
            }),
            "A" => {
                let k = unesc(parts.next().unwrap());
                let v = unesc(parts.next().unwrap_or(""));
                stack
                    .last_mut()
                    .unwrap()
                    .attrib
                    .insert(LStr::from(k.as_str()), LStr::from(v.as_str()));
            }
            "T" => {
                let t = unesc(parts.next().unwrap_or(""));
                stack
                    .last_mut()
                    .unwrap()
                    .children
                    .push(XmlChild::Text(LStr::from(t.as_str())));
            }
            "/" => {
                let n = stack.pop().unwrap();
                match stack.last_mut() {
                    Some(p) => p.children.push(XmlChild::Node(n)),
                    None => top.push(n),
                }
            }
            other => panic!("bad xml record {other:?}"),
        }
    }
    top
}

// ---- jewels --------------------------------------------------------------------------------

/// A jewel as PS-B reads it (the serialised oracle item, or a synthetic stand-in).
#[derive(Clone, Debug, Default)]
struct Jewel {
    id: f64,
    radius: Option<f64>,
    has_jd: bool,
    /// (seed, conqueror type, conqueror id: number or string)
    conq: Option<(f64, String, ConqId)>,
    keystones: Option<Vec<String>>,
    /// scalar jewelData fields
    fields: Vec<(String, Val)>,
}

#[derive(Clone, Debug)]
enum ConqId {
    N(f64),
    S(String),
}

impl Jewel {
    /// The port's item. `conqueredBy` is an immutable data table, as IT-C builds it
    /// (`jewelData[value.key] = value.value` of a parsed `JewelData` mod).
    fn item(&self) -> Item {
        let jd = self.has_jd.then(|| {
            let t = Table::new();
            for (k, v) in &self.fields {
                t.set(Name::from(k.as_str()), v.clone());
            }
            if let Some((seed, ty, cid)) = &self.conq {
                let mut conqueror = ValueTable::new();
                conqueror.set(Name::from("type"), Value::Str(ty.as_str().into()));
                conqueror.set(
                    Name::from("id"),
                    match cid {
                        ConqId::N(x) => Value::Num(*x),
                        ConqId::S(s) => Value::Str(s.as_str().into()),
                    },
                );
                let mut cb = ValueTable::new();
                cb.set(Name::from("id"), Value::Num(*seed));
                cb.set(Name::from("conqueror"), Value::Table(Arc::new(conqueror)));
                t.set(n("conqueredBy"), Val::Data(Value::Table(Arc::new(cb))));
            }
            if let Some(keys) = &self.keystones {
                let k = Table::new();
                for name in keys {
                    k.set(Name::from(name.as_str()), true);
                }
                t.set(n("impossibleEscapeKeystones"), k);
            }
            t
        });
        Item {
            id: std::cell::Cell::new(Some(self.id)),
            jewelRadiusIndex: std::cell::Cell::new(self.radius),
            jewelData: Field::new(jd),
            ..Item::default()
        }
    }

    /// The oracle's stand-in item fields (`{ radius, jd }` as Lua constructors).
    fn lua_jd(&self) -> String {
        let mut p: Vec<String> = Vec::new();
        for (k, v) in &self.fields {
            let v = match v {
                Val::Bool(b) => b.to_string(),
                Val::Num(x) => tostring_num(*x),
                Val::Str(s) => format!("{:?}", &**s),
                other => panic!("field {k}: {other:?}"),
            };
            p.push(format!("[{k:?}] = {v}"));
        }
        if let Some((seed, ty, cid)) = &self.conq {
            let cid = match cid {
                ConqId::N(x) => tostring_num(*x),
                ConqId::S(s) => format!("{s:?}"),
            };
            p.push(format!(
                "conqueredBy = {{ id = {}, conqueror = {{ type = {ty:?}, id = {cid} }} }}",
                tostring_num(*seed)
            ));
        }
        if let Some(keys) = &self.keystones {
            let ks: Vec<String> = keys.iter().map(|k| format!("[{k:?}] = true")).collect();
            p.push(format!(
                "impossibleEscapeKeystones = {{ {} }}",
                ks.join(", ")
            ));
        }
        format!("{{ {} }}", p.join(", "))
    }
}

fn n(s: &str) -> Name {
    Name::from(s)
}

fn parse_items(s: &str) -> BTreeMap<u32, Jewel> {
    let mut out: BTreeMap<u32, Jewel> = BTreeMap::new();
    for line in s.split('\n').filter(|l| !l.is_empty()) {
        let f: Vec<String> = line.split('\t').map(unesc).collect();
        let id: f64 = f[1].parse().unwrap();
        let j = out.entry(id as u32).or_default();
        match f[0].as_str() {
            "I" => {
                j.id = id;
                j.radius = f[2].parse().ok();
                j.has_jd = f[3] == "true";
            }
            "C" => {
                let cid = if f[4] == "number" {
                    ConqId::N(f[5].parse().unwrap())
                } else {
                    ConqId::S(f[5].clone())
                };
                j.conq = Some((f[2].parse().unwrap(), f[3].clone(), cid));
            }
            "K" => {
                j.keystones = Some(
                    f[2].split('\u{1}')
                        .filter(|k| !k.is_empty())
                        .map(str::to_owned)
                        .collect(),
                );
            }
            "J" => {
                let v = match f[3].as_str() {
                    "boolean" => Val::Bool(f[4] == "true"),
                    "number" => Val::Num(f[4].parse().unwrap()),
                    _ => Val::from(f[4].as_str()),
                };
                j.fields.push((f[2].clone(), v));
            }
            other => panic!("bad item record {other}"),
        }
    }
    out
}

/// One synthetic build: a corpus spec with `jewel` in socket `socket`.
#[derive(Clone, Debug)]
struct Synth {
    file: usize,
    spec: usize,
    socket: f64,
    jewel: Jewel,
    label: String,
}

impl Synth {
    fn lua(&self) -> String {
        format!(
            "{{ socket = {}, itemId = {}, radius = {}, jd = {} }}",
            tostring_num(self.socket),
            tostring_num(self.jewel.id),
            self.jewel
                .radius
                .map_or_else(|| "nil".to_owned(), tostring_num),
            self.jewel.lua_jd()
        )
    }
}

/// The 62 synthetic jewels (the calc doc asks for 60): every conqueror of every timeless and abyss type (each with two
/// seeds), an out-of-range seed, a `limitDisabled` one, and intuitive-leap / impossible
/// escape / split personality stand-ins.
fn synthetic_jewels() -> Vec<(String, Jewel)> {
    let mut out = Vec::new();
    let legion: [(&str, &[ConqId], f64, f64); 6] = [
        (
            "vaal",
            &[
                ConqId::N(1.0),
                ConqId::N(2.0),
                ConqId::N(3.0),
                ConqId::S("2_v2".into()),
            ],
            100.0,
            8000.0,
        ),
        (
            "karui",
            &[
                ConqId::N(1.0),
                ConqId::N(2.0),
                ConqId::N(3.0),
                ConqId::S("3_v2".into()),
            ],
            10000.0,
            18000.0,
        ),
        (
            "maraketh",
            &[
                ConqId::N(1.0),
                ConqId::N(2.0),
                ConqId::N(3.0),
                ConqId::S("1_v2".into()),
            ],
            500.0,
            8000.0,
        ),
        (
            "templar",
            &[
                ConqId::N(1.0),
                ConqId::N(2.0),
                ConqId::N(3.0),
                ConqId::S("1_v2".into()),
            ],
            2000.0,
            10000.0,
        ),
        (
            "eternal",
            &[
                ConqId::N(1.0),
                ConqId::N(2.0),
                ConqId::N(3.0),
                ConqId::S("3_v2".into()),
            ],
            2000.0,
            160_000.0,
        ),
        (
            "kalguur",
            &[ConqId::N(1.0), ConqId::N(2.0), ConqId::N(3.0)],
            100.0,
            8000.0,
        ),
    ];
    let mut k = 0usize;
    let mut seed_at = |lo: f64, hi: f64, step: f64| {
        k += 1;
        let frac = ((k * 7919) % 1000) as f64 / 1000.0;
        let s = lo + ((hi - lo) * frac / step).floor() * step;
        s.clamp(lo, hi)
    };
    for (ty, ids, lo, hi) in legion {
        let step = if ty == "eternal" { 20.0 } else { 1.0 };
        for cid in ids {
            for _ in 0..2 {
                let seed = seed_at(lo, hi, step);
                out.push((
                    format!("{ty}/{cid:?}/{seed}"),
                    Jewel {
                        radius: Some(3.0),
                        has_jd: true,
                        conq: Some((seed, ty.to_owned(), cid.clone())),
                        keystones: Some(Vec::new()),
                        ..Jewel::default()
                    },
                ));
            }
        }
    }
    for ty in [
        "abyss_murderous",
        "abyss_searching",
        "abyss_hypnotic",
        "abyss_ghastly",
        "abyss_special",
    ] {
        for _ in 0..2 {
            let seed = seed_at(100.0, 8000.0, 1.0);
            out.push((
                format!("{ty}/{seed}"),
                Jewel {
                    radius: None,
                    has_jd: true,
                    conq: Some((seed, ty.to_owned(), ConqId::N(1.0))),
                    keystones: Some(Vec::new()),
                    ..Jewel::default()
                },
            ));
        }
    }
    // out of range (ConPrintf), limitDisabled
    out.push((
        "vaal/out-of-range".into(),
        Jewel {
            radius: Some(3.0),
            has_jd: true,
            conq: Some((9000.0, "vaal".into(), ConqId::N(1.0))),
            keystones: Some(Vec::new()),
            ..Jewel::default()
        },
    ));
    out.push((
        "karui/limitDisabled".into(),
        Jewel {
            radius: Some(3.0),
            has_jd: true,
            conq: Some((12000.0, "karui".into(), ConqId::N(1.0))),
            keystones: Some(Vec::new()),
            fields: vec![("limitDisabled".into(), Val::Bool(true))],
            ..Jewel::default()
        },
    ));
    out.push((
        "intuitive leap".into(),
        Jewel {
            radius: Some(1.0),
            has_jd: true,
            keystones: Some(Vec::new()),
            fields: vec![("intuitiveLeapLike".into(), Val::Bool(true))],
            ..Jewel::default()
        },
    ));
    out.push((
        "intuitive leap keystone only".into(),
        Jewel {
            radius: Some(3.0),
            has_jd: true,
            keystones: Some(Vec::new()),
            fields: vec![
                ("intuitiveLeapLike".into(), Val::Bool(true)),
                ("intuitiveLeapKeystoneOnly".into(), Val::Bool(true)),
            ],
            ..Jewel::default()
        },
    ));
    out.push((
        "impossible escape".into(),
        Jewel {
            radius: Some(1.0),
            has_jd: true,
            keystones: Some(vec!["Resolute Technique".into()]),
            fields: vec![
                ("impossibleEscapeKeystone".into(), Val::Bool(true)),
                ("intuitiveLeapLike".into(), Val::Bool(true)),
            ],
            ..Jewel::default()
        },
    ));
    out.push((
        "split personality".into(),
        Jewel {
            radius: None,
            has_jd: true,
            keystones: Some(Vec::new()),
            fields: vec![("jewelIncEffectFromClassStart".into(), Val::Bool(true))],
            ..Jewel::default()
        },
    ));
    out
}

// ---- the port's PS-C stand-ins ---------------------------------------------------------------

/// Records PS-B's calls into PS-C and its `ConPrintf` (the oracle's `rec` lines).
struct Recorder {
    trace: RefCell<Vec<String>>,
}

fn id_text(node: &Node) -> String {
    match node_id_val(node) {
        Val::Num(x) => tostring_num(x),
        Val::Str(s) => s.to_string(),
        other => format!("{other:?}"),
    }
}

impl PscCalls for Recorder {
    fn ReplaceNode(&self, _spec: &PassiveSpec, old: &NodeRef, newNode: &NodeRef) {
        self.trace.borrow_mut().push(format!(
            "R\t{}\t{}\t{}",
            id_text(old),
            id_text(newNode),
            newNode.dn.get()
        ));
        if old.sd.get().ptr_eq(&newNode.sd.get()) {
            return;
        }
        // `old.x = newNode.x`; a nil value falls through to the tree node (Lua `__index`)
        let t = old.tree.clone();
        old.dn.set(newNode.dn.get());
        old.sd.set(newNode.sd.get());
        let name = newNode.extra.get(n("name"));
        let name = if name.is_nil() {
            t.as_ref().map_or(Val::Nil, |t| t.extra.get(n("name")))
        } else {
            name
        };
        old.extra.set(n("name"), name);
        old.isTattoo.set(
            newNode.isTattoo.get()
                || (!newNode.isTattoo.get() && t.as_ref().is_some_and(|t| t.isTattoo.get())),
        );
        old.overrideType.set(
            newNode
                .overrideType
                .get()
                .or_else(|| t.as_ref().and_then(|t| t.overrideType.get())),
        );
    }

    fn NodeAdditionOrReplacementFromString(
        &self,
        _spec: &PassiveSpec,
        node: &NodeRef,
        sd: &str,
        replacement: bool,
    ) {
        self.trace
            .borrow_mut()
            .push(format!("A\t{}\t{sd}\t{replacement}", id_text(node)));
    }

    fn ReconnectNodeToClassStart(&self, _spec: &PassiveSpec, node: &NodeRef) {
        self.trace
            .borrow_mut()
            .push(format!("C\t{}", id_text(node)));
    }

    /// PassiveSpec.lua:2485-2494.
    fn NodeInKeystoneRadius(
        &self,
        spec: &PassiveSpec,
        keystoneNames: &Table,
        nodeId: f64,
        radiusIndex: f64,
    ) -> bool {
        let tree = spec.tree.get();
        for (k, _) in keystoneNames.pairs() {
            let Key::S(name) = k else { continue };
            let Some(ks) = tree.keystoneMap.get(name).obj::<Node>() else {
                continue;
            };
            if radiusIndex.is_nan() {
                continue;
            }
            if nodesInRadiusAt(&ks, radiusIndex).is_some_and(|m| m.contains_key(nodeId as i64)) {
                return true;
            }
        }
        false
    }

    fn ConPrintf(&self, msg: &str) {
        self.trace.borrow_mut().push(format!("P\t{msg}"));
    }
}

// ---- the port side ---------------------------------------------------------------------------

fn ids(list: Option<Vec<NodeRef>>) -> String {
    match list {
        None => "nil".to_owned(),
        Some(v) => v.iter().map(|n| id_text(n)).collect::<Vec<_>>().join(","),
    }
}

fn val_text(v: &Val) -> String {
    match v {
        Val::Nil => "nil".to_owned(),
        Val::Bool(b) => b.to_string(),
        Val::Num(x) => tostring_num(*x),
        Val::Str(s) => s.to_string(),
        other => format!("{other:?}"),
    }
}

fn conq_text(c: Option<Table>) -> String {
    let Some(c) = c else {
        return "nil".to_owned();
    };
    let cv = Val::Table(c.clone());
    let conqueror = cv.get(n("conqueror"));
    let (ty, id) = if conqueror.truthy() {
        (conqueror.get(n("type")), conqueror.get(n("id")))
    } else {
        (Val::Nil, Val::Nil)
    };
    let mut parts = vec![val_text(&cv.get(n("id"))), val_text(&ty), val_text(&id)];
    let m = cv.get(n("modification"));
    if m.truthy() {
        let Val::Data(Value::Table(m)) = m else {
            panic!("modification {m:?}")
        };
        for comp in &m.array {
            let comp = Val::from_value(comp);
            let rolls: Vec<String> = match comp.get(n("rolls")) {
                Val::Data(Value::Table(r)) => r
                    .array
                    .iter()
                    .map(|x| val_text(&Val::from_value(x)))
                    .collect(),
                _ => Vec::new(),
            };
            parts.push(format!(
                "{}/{}/{}",
                val_text(&comp.get(n("type"))),
                val_text(&comp.get(n("id"))),
                rolls.join("/")
            ));
        }
    }
    parts.join(":")
}

fn kv_sorted(t: &Table) -> String {
    let mut p: Vec<(String, String)> = t
        .pairs()
        .map(|(k, v)| (val_text(&k.to_val()), val_text(&v)))
        .collect();
    p.sort();
    p.into_iter()
        .map(|(k, v)| format!("{k}={v}"))
        .collect::<Vec<_>>()
        .join(",")
}

fn sorted_num_keys(t: &Table) -> String {
    let mut ks: Vec<f64> = t
        .pairs()
        .filter_map(|(k, _)| match k {
            Key::I(i) => Some(i as f64),
            _ => None,
        })
        .collect();
    ks.sort_by(f64::total_cmp);
    ks.into_iter()
        .map(tostring_num)
        .collect::<Vec<_>>()
        .join(",")
}

fn list_strs(v: &Val) -> String {
    match v {
        Val::Table(t) => t
            .ipairs()
            .into_iter()
            .map(|(_, s)| val_text(&s))
            .collect::<Vec<_>>()
            .join("/"),
        _ => String::new(),
    }
}

fn state(spec: &PassiveSpecRef, rec: &Recorder, step: &str, r: &mut Rec) {
    let keys = |m: &LjIntMap<NodeRef>| {
        m.keys()
            .into_iter()
            .map(|k| k.to_string())
            .collect::<Vec<_>>()
            .join(",")
    };
    r.emit(format!("{step}/alloc"), spec.allocNodes.with(keys));
    r.emit(
        format!("{step}/mastery"),
        spec.masterySelections
            .with(LjIntMap::pairs)
            .iter()
            .map(|(k, v)| format!("{k}={}", tostring_num(*v)))
            .collect::<Vec<_>>()
            .join(","),
    );
    r.emit(
        format!("{step}/counts"),
        [
            tostring_num(spec.allocatedMasteryCount.get()),
            tostring_num(spec.allocatedNotableCount.get()),
            tostring_num(spec.allocatedKeystoneCount.get()),
            tostring_num(spec.allocatedMasteryTypeCount.get()),
            kv_sorted(&spec.allocatedMasteryTypes),
            kv_sorted(&spec.allocatedTattooTypes),
        ]
        .join("|"),
    );
    r.emit(
        format!("{step}/split"),
        spec.splitPersonalityPath
            .get()
            .map_or_else(String::new, |t| sorted_num_keys(&t)),
    );
    r.emit(
        format!("{step}/ignored"),
        sorted_num_keys(&spec.ignoredNodes),
    );
    r.emit(
        format!("{step}/cnl"),
        spec.extra
            .get(n("checkNodeLinks"))
            .raw_eq(&Val::Bool(true))
            .to_string(),
    );
    r.emit(
        format!("{step}/trace"),
        std::mem::take(&mut *rec.trace.borrow_mut()).join("\n"),
    );
    let tree = spec.tree.get();
    let mut nodes = spec.nodes.with(LjIntMap::pairs);
    nodes.sort_by_key(|(k, _)| *k);
    for (id, node) in nodes {
        let ill = match node.extra.get(n("intuitiveLeapLikesAffecting")) {
            Val::Table(t) => Some(
                t.ipairs()
                    .into_iter()
                    .map(|(_, v)| v.obj::<Node>().expect("node"))
                    .collect(),
            ),
            _ => None,
        };
        let mut parts = vec![
            node.alloc.get().to_string(),
            ids(Some(node.depends.get().snapshot())),
            ids(ill),
            conq_text(node.conqueredBy.get()),
            node.pathDist
                .get()
                .map_or_else(|| "nil".to_owned(), tostring_num),
            ids(node.path.get().map(|p| p.snapshot())),
            node.distanceToClassStart
                .get()
                .map_or_else(|| "nil".to_owned(), tostring_num),
            node.connectedToStart.get().to_string(),
            node.extra
                .get(n("visited"))
                .raw_eq(&Val::Bool(true))
                .to_string(),
            node.allMasteryOptions.get().to_string(),
        ];
        let t = tree.nodes.get(id);
        if &*node.r#type.get() == "Mastery"
            || t.as_ref()
                .is_none_or(|t| !t.sd.get().ptr_eq(&node.sd.get()))
        {
            let sd = node
                .sd
                .get()
                .snapshot()
                .iter()
                .map(|s| s.to_string())
                .collect::<Vec<_>>()
                .join("/");
            parts.push(format!(
                "{sd}#{}#{}",
                list_strs(&node.extra.get(n("reminderText"))),
                node.dn.get()
            ));
        }
        r.emit(format!("{step}/n/{id}"), parts.join("|"));
    }
}

/// `spec.nodes` rebuilt with the keys in ascending order (the oracle does the same).
fn renorm_spec(spec: &PassiveSpecRef) {
    spec.nodes.with_mut(|m| {
        let mut ks = m.keys();
        ks.sort_unstable();
        let mut nm = LjIntMap::new();
        for k in ks {
            nm.set(k, m.get(k));
        }
        *m = nm;
    });
}

/// Every tree node's `nodesInRadius[r]` rebuilt with the keys in ascending order.
fn renorm_tree(tree: &PassiveTreeRef) {
    for (_, node) in tree.nodes.pairs() {
        let Some(nir) = node.nodesInRadius.get() else {
            continue;
        };
        for (r, v) in nir.pairs().collect::<Vec<_>>() {
            let Some(m) = v.obj::<LjIntMap<NodeRef>>() else {
                continue;
            };
            let mut ks = m.keys();
            ks.sort_unstable();
            let mut nm = LjIntMap::new();
            for k in ks {
                nm.set(k, m.get(k));
            }
            nir.set(r, Val::Obj(Rc::new(nm)));
        }
    }
}

fn tree_version_of(node: &XmlNode) -> Name {
    node.attrib.get("treeVersion").map_or_else(
        || data().gameVersions.defaultTreeVersion,
        |s| Name::from(&**s),
    )
}

fn rust_run(
    xml: &XmlNode,
    jewels: &BTreeMap<u32, Jewel>,
    synth: Option<&Synth>,
    ops: &str,
    rec: &Rc<Recorder>,
) -> BTreeMap<String, String> {
    let mut r = Rec(BTreeMap::new());
    let build: BuildRef = Build::new(data());
    build.itemsTab.items.with_mut(|m| {
        for (id, j) in jewels {
            m.insert(*id, Rc::new(j.item()));
        }
    });
    let ver = tree_version_of(xml);
    let spec = new_PassiveSpec(&build, ver, false);
    renorm_spec(&spec);
    rec.trace.borrow_mut().clear();
    spec.Load(xml, "corpus");
    state(&spec, rec, "load", &mut r);
    if let Some(s) = synth {
        build
            .itemsTab
            .items
            .with_mut(|m| m.insert(s.jewel.id as u32, Rc::new(s.jewel.item())));
        spec.jewels
            .with_mut(|m| m.set(s.socket as i64, Some(s.jewel.id)));
        spec.BuildAllDependsAndPaths();
        state(&spec, rec, "synth", &mut r);
    }
    spec.BuildAllDependsAndPaths();
    state(&spec, rec, "again", &mut r);
    for op in ops.split(',').filter(|o| !o.is_empty()) {
        let (k, id) = op[1..].split_once(':').unwrap();
        let id: i64 = id.parse().unwrap();
        let node = spec.nodes.with(|m| m.get(id)).expect("op node");
        if op.starts_with('D') {
            spec.DeallocNode(&node);
            state(&spec, rec, &format!("dealloc{k}"), &mut r);
        } else {
            spec.AllocNode(&node, None);
            state(&spec, rec, &format!("alloc{k}"), &mut r);
        }
    }
    r.emit("ops", ops);
    r.0
}

/// The allocated sockets of a spec (as the port loads it), ascending.
fn allocated_sockets(xml: &XmlNode, jewels: &BTreeMap<u32, Jewel>, rec: &Rc<Recorder>) -> Vec<f64> {
    let build: BuildRef = Build::new(data());
    build.itemsTab.items.with_mut(|m| {
        for (id, j) in jewels {
            m.insert(*id, Rc::new(j.item()));
        }
    });
    let spec = new_PassiveSpec(&build, tree_version_of(xml), false);
    spec.Load(xml, "corpus");
    rec.trace.borrow_mut().clear();
    let mut out: Vec<f64> = spec
        .allocNodes
        .with(LjIntMap::pairs)
        .into_iter()
        .filter(|(_, n)| &*n.r#type.get() == "Socket" && n.expansionJewel.get().is_none())
        .map(|(k, _)| k as f64)
        .collect();
    out.sort_by(f64::total_cmp);
    out
}

/// Record kind for the summary: the step name without its number, the record name.
fn kind_of(k: &str) -> String {
    let mut parts = k.split('/');
    let step: String = parts
        .next()
        .unwrap_or("")
        .trim_end_matches(|c: char| c.is_ascii_digit())
        .to_owned();
    let rest: Vec<&str> = parts.filter(|p| p.parse::<f64>().is_err()).collect();
    if rest.is_empty() {
        step
    } else {
        format!("{step}/{}", rest.join("/"))
    }
}

/// The first differing line of two multi-line records.
fn first_diff(l: &str, r: &str) -> String {
    let (l, r) = (unesc(l), unesc(r));
    let (ll, rl): (Vec<&str>, Vec<&str>) = (l.split('\n').collect(), r.split('\n').collect());
    for i in 0..ll.len().max(rl.len()) {
        let (a, b) = (ll.get(i), rl.get(i));
        if a != b {
            return format!(
                "line {} of {}/{}:\n    lua  {:?}\n    rust {:?}",
                i + 1,
                ll.len(),
                rl.len(),
                a,
                b
            );
        }
    }
    "identical lines".to_owned()
}

#[derive(Default)]
struct Totals {
    total: BTreeMap<String, usize>,
    bad: BTreeMap<String, usize>,
    shown: usize,
    runs: usize,
    abyss_nodes: usize,
    conquered_nodes: usize,
    trace_lines: BTreeMap<char, usize>,
    con_lines: BTreeMap<String, usize>,
    tolerated: BTreeMap<String, usize>,
}

/// `s` with every run of digits (and decimal points inside one) replaced by `#`.
fn mask_digits(s: &str) -> String {
    let mut out = String::new();
    let mut prev = false;
    for c in s.chars() {
        let d = c.is_ascii_digit() || (prev && c == '.');
        if !(d && prev) {
            out.push(if d { '#' } else { c });
        }
        prev = d;
    }
    out
}

/// A trace that differs from the oracle's only in `NodeAdditionOrReplacementFromString` lines of
/// abyss-conquered nodes, and there only in numbers: the roll a multi-stat legion node's line
/// gets depends on PoB's `pairs(changedNode.stats)` order when two stats share a `(min-max)`
/// text (PassiveSpec.lua:1261; e.g. "Minions have (2-3)% increased Attack / Cast Speed"),
/// which is random per process in PoB (string keys; CCR-0070 §4).
fn abyss_order_only(
    k: &str,
    l: Option<&String>,
    r: Option<&String>,
    lua: &BTreeMap<String, String>,
) -> bool {
    let (Some(l), Some(r)) = (l, r) else {
        return false;
    };
    let step = k.split('/').next().unwrap_or("");
    let (l, r) = (unesc(l), unesc(r));
    let (ll, rl): (Vec<&str>, Vec<&str>) = (l.split('\n').collect(), r.split('\n').collect());
    if ll.len() != rl.len() {
        return false;
    }
    ll.iter().zip(&rl).filter(|(a, b)| a != b).all(|(a, b)| {
        let (fa, fb): (Vec<&str>, Vec<&str>) = (a.split('\t').collect(), b.split('\t').collect());
        fa.len() == 4
            && fb.len() == 4
            && fa[0] == "A"
            && fb[0] == "A"
            && fa[1] == fb[1]
            && fa[3] == fb[3]
            && mask_digits(fa[2]) == mask_digits(fb[2])
            && lua
                .get(&format!("{step}/n/{}", fa[1]))
                .and_then(|v| v.split('|').nth(3))
                .is_some_and(|c| c.matches(':').count() > 2)
    })
}

impl Totals {
    fn compare(
        &mut self,
        label: &str,
        lua: &BTreeMap<String, String>,
        rust: &BTreeMap<String, String>,
    ) -> usize {
        self.runs += 1;
        let keys: BTreeSet<&String> = lua.keys().chain(rust.keys()).collect();
        let mut bad = 0usize;
        for k in keys {
            let kind = kind_of(k);
            let (l, r) = (lua.get(k), rust.get(k));
            *self.total.entry(kind.clone()).or_default() += 1;
            if l != r && k.ends_with("/trace") && abyss_order_only(k, l, r, lua) {
                // PoB's random `pairs(changedNode.stats)` order (CCR-0070 §4)
                eprintln!(
                    "{label} {k}: differs only in the roll PoB picks through its random stats order: {}",
                    first_diff(l.map_or("", |s| s), r.map_or("", |s| s))
                );
                *self.tolerated.entry(kind).or_default() += 1;
            } else if l != r {
                *self.bad.entry(kind).or_default() += 1;
                bad += 1;
                if self.shown < 30 {
                    self.shown += 1;
                    let cut = |s: Option<&String>| {
                        s.map_or_else(|| "<missing>".to_owned(), |s| s.chars().take(600).collect())
                    };
                    if k.ends_with("/trace") {
                        eprintln!(
                            "{label} MISMATCH {k}: {}",
                            first_diff(l.map_or("", |s| s), r.map_or("", |s| s))
                        );
                    } else {
                        eprintln!("{label} MISMATCH {k}\n  lua  {}\n  rust {}", cut(l), cut(r));
                    }
                }
            }
        }
        // coverage counters (from the oracle's records)
        for (k, v) in lua {
            if k.contains("/n/") {
                let f: Vec<&str> = v.split('|').collect();
                if f.get(3).is_some_and(|c| *c != "nil") {
                    self.conquered_nodes += 1;
                    if f[3].matches(':').count() > 2 {
                        self.abyss_nodes += 1;
                    }
                }
            } else if k.ends_with("/trace") {
                for line in unesc(v).split('\n').filter(|l| !l.is_empty()) {
                    *self
                        .trace_lines
                        .entry(line.chars().next().unwrap())
                        .or_default() += 1;
                    if let Some(msg) = line.strip_prefix("P\t") {
                        let head = msg.split(':').next().unwrap_or(msg).to_owned();
                        *self.con_lines.entry(head).or_default() += 1;
                    }
                }
            }
        }
        bad
    }
}

fn lua_str(v: Result<Vec<Ret>, String>, what: &str) -> Vec<String> {
    match v {
        Ok(v) => v
            .into_iter()
            .map(|r| match r {
                Ret::Str(s) => s,
                other => panic!("{what}: oracle returned {other:?}"),
            })
            .collect(),
        Err(e) => panic!("{what}: oracle: {e}"),
    }
}

#[test]
fn build_all_depends_and_paths_acceptance() {
    let Some(src) = find_pob_src() else {
        return skip("no PoB checkout");
    };
    if !init_data() {
        return skip("data/dump not found");
    }
    let Some(parity) = find_parity_dir() else {
        return skip("no .parity corpus");
    };
    let src = std::fs::canonicalize(&src).unwrap();
    lut::set_jewel_fs(Arc::new(lut::StdJewelFs {
        script_path: src.to_string_lossy().into_owned(),
        writable: false,
    }));
    lut::set_con_printf(|_| {});
    let only: Option<Vec<String>> = std::env::var("PSB_BUILDS")
        .ok()
        .map(|s| s.split(',').map(|v| v.trim().to_owned()).collect());
    let with_synth = std::env::var("PSB_SYNTH").map_or(true, |v| v != "0");
    let mut files: Vec<PathBuf> = std::fs::read_dir(parity.join("builds"))
        .unwrap()
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().is_some_and(|e| e == "xml"))
        .collect();
    files.sort();
    if files.is_empty() {
        return skip("no builds in .parity/builds");
    }

    let t0 = std::time::Instant::now();
    let mut pob = JitPob::boot_oracle(&src, Some(PRE)).unwrap_or_else(|e| panic!("boot: {e}"));
    pob.run(LIB).unwrap_or_else(|e| panic!("{e}"));
    eprintln!("oracle boot: {:.1}s", t0.elapsed().as_secs_f64());

    let rec = Rc::new(Recorder {
        trace: RefCell::new(Vec::new()),
    });
    set_psc_calls(Some(rec.clone() as Rc<dyn PscCalls>));
    // the port side of the oracle's PS-C no-ops outside BuildAllDependsAndPaths (LIB): inside it
    // `ReplaceNode` goes to the recorder above, never to the port's method (CCR-0052 §12)
    set_spec_stubs(SpecStubs {
        build_cluster_jewel_graphs: true,
        replace_node: true,
        ..SpecStubs::NONE
    });
    let mut renormed: BTreeSet<String> = BTreeSet::new();
    let mut ensure_tree = |ver: Name| {
        if renormed.insert(ver.to_string()) {
            let tree = main_LoadTree(ver).expect("tree version");
            renorm_tree(&tree);
        }
        let _ = main_LoadTree(ver);
    };

    let synth_jewels = if with_synth {
        synthetic_jewels()
    } else {
        Vec::new()
    };
    let mut synths: Vec<Synth> = Vec::new();
    let mut tot = Totals::default();
    let mut specs = 0usize;

    let selected = |p: &PathBuf| {
        only.as_ref().is_none_or(|o| {
            o.iter().any(|s| {
                p.file_name()
                    .unwrap()
                    .to_string_lossy()
                    .contains(s.as_str())
            })
        })
    };
    // pass 1: the corpus (and the socket choice of the synthetic builds)
    let mut synth_pool: Vec<(usize, usize, Vec<f64>)> = Vec::new();
    for (fi, file) in files.iter().enumerate() {
        let stem = file.file_stem().unwrap().to_string_lossy().to_string();
        if !selected(file) && !with_synth {
            continue;
        }
        let text = std::fs::read_to_string(file).unwrap();
        let r = lua_str(pob.run(&format!("return __psb.load({text:?})")), &stem);
        let nodes = parse_xml(&r[0]);
        let jewels = parse_items(&r[1]);
        for (si, node) in nodes.iter().enumerate() {
            let ver = tree_version_of(node);
            ensure_tree(ver);
            if with_synth && &*ver == "3_29" && si == 0 {
                let socks = allocated_sockets(node, &jewels, &rec);
                if !socks.is_empty() {
                    synth_pool.push((fi, si, socks));
                }
            }
            if !selected(file) {
                continue;
            }
            specs += 1;
            let lua = parse_records(
                &lua_str(
                    pob.run(&format!("return __psb.run({}, nil)", si + 1)),
                    &stem,
                )[0],
            );
            let ops = unesc(lua.get("ops").map_or("", |s| s));
            let rust = rust_run(node, &jewels, None, &ops, &rec);
            let bad = tot.compare(&format!("{stem}#{si}"), &lua, &rust);
            eprintln!(
                "{stem}#{si} ({ver}, {} jewels): {} records, {bad} mismatches",
                jewels.values().filter(|j| j.has_jd).count(),
                lua.len()
            );
        }
        let _ = pob.run("__psb.clearLUTs()");
        lut::clear_timeless_jewel_luts(None);
    }

    // pass 2: the synthetic builds (a corpus spec, one stand-in jewel in an allocated socket)
    if !synth_pool.is_empty() {
        for (k, (label, jewel)) in synth_jewels.into_iter().enumerate() {
            let (fi, si, socks) = &synth_pool[k % synth_pool.len()];
            let socket = socks[(k / synth_pool.len()) % socks.len()];
            synths.push(Synth {
                file: *fi,
                spec: *si,
                socket,
                jewel: Jewel {
                    id: 9001.0 + k as f64,
                    ..jewel
                },
                label,
            });
        }
        synths.sort_by_key(|s| s.file);
        let mut cur: Option<usize> = None;
        let mut cache: Option<(Vec<XmlNode>, BTreeMap<u32, Jewel>)> = None;
        for s in &synths {
            let file = &files[s.file];
            let stem = file.file_stem().unwrap().to_string_lossy().to_string();
            if cur != Some(s.file) {
                let text = std::fs::read_to_string(file).unwrap();
                let r = lua_str(pob.run(&format!("return __psb.load({text:?})")), &stem);
                cache = Some((parse_xml(&r[0]), parse_items(&r[1])));
                cur = Some(s.file);
            }
            let (nodes, jewels) = cache.as_ref().unwrap();
            let node = &nodes[s.spec];
            ensure_tree(tree_version_of(node));
            let lua = parse_records(
                &lua_str(
                    pob.run(&format!("return __psb.run({}, {})", s.spec + 1, s.lua())),
                    &stem,
                )[0],
            );
            let ops = unesc(lua.get("ops").map_or("", |s| s));
            let rust = rust_run(node, jewels, Some(s), &ops, &rec);
            let label = format!("synth[{}] {} @{} in {stem}", s.label, s.jewel.id, s.socket);
            let bad = tot.compare(&label, &lua, &rust);
            eprintln!("{label}: {} records, {bad} mismatches", lua.len());
            let _ = pob.run("__psb.clearLUTs()");
            lut::clear_timeless_jewel_luts(None);
        }
    }
    set_psc_calls(None);
    set_spec_stubs(SpecStubs::NONE);

    eprintln!(
        "\n== PS-B: {specs} corpus specs, {} synthetic builds, {} runs ==",
        synths.len(),
        tot.runs
    );
    for (k, nrec) in &tot.total {
        let b = tot.bad.get(k).copied().unwrap_or(0);
        eprintln!("  {k:<22} {:>8}/{nrec:<8} identical", nrec - b);
    }
    eprintln!(
        "  coverage: {} conquered node records ({} abyss), trace lines {:?}",
        tot.conquered_nodes, tot.abyss_nodes, tot.trace_lines
    );
    eprintln!("  ConPrintf lines: {:?}", tot.con_lines);
    eprintln!(
        "  traces differing only by PoB's random abyss stats order (tolerated): {:?}",
        tot.tolerated
    );
    let bad: usize = tot.bad.values().sum();
    let total: usize = tot.total.values().sum();
    eprintln!("  records: {}/{total} identical", total - bad);
    assert!(
        with_synth.then_some(synths.len()).is_none_or(|n| n == 62),
        "synthetic builds: {}",
        synths.len()
    );
    assert_eq!(bad, 0, "{bad} mismatching records (see above)");
}
