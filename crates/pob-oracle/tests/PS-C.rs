//! Unit PS-C acceptance (calc doc §11.1, row PS-C: "L2 subgraph ids, mods and legacy remap on
//! cluster builds"): `pob_calc::classes::passive_spec_cluster` (Classes/PassiveSpec.lua
//! 1697-2494) against PoB's own `PassiveSpec` class under LuaJIT (interpreter,
//! `JitPob::boot_oracle`).
//!
//! `BuildAllDependsAndPaths` (PS-B) is a no-op on both sides: the oracle replaces it (it only
//! initialises the mastery counters PS-B owns), the port sets `passive_spec::set_spec_stubs`
//! (CCR-0052 §12; PS-B has its own test); every other PassiveSpec function runs for real on
//! both sides.
//!
//! Builds: every build of the all57 corpus (`.parity/builds`) whose items include a cluster
//! jewel (among them three legacy, v1-hash builds: 3_6 and 3_13 trees without
//! `clusterHashFormatVersion`), plus synthetic builds made here on the 3_29 and 3_13 trees:
//! each large socket holds a Large Cluster Jewel, its sub-sockets Medium ones and theirs Small
//! ones (or, downsized, a Medium with Smalls, or a Small, in the large socket), across node /
//! socket / notable counts (both Medium special rules), "grants nothing" smalls, added mods and
//! increased effect, a keystone cluster and a missing notable; each synthetic build also runs as
//! a legacy (`clusterHashFormatVersion="1"`) spec with every candidate cluster id allocated.
//! Legacy specs also run the conversion step by step (`BeginLegacyClusterHashConversion`,
//! `BuildSubgraph` per large socket, then the forward / reverse maps, `ApplyLegacyClusterNodeRemap`
//! and `EndLegacyClusterHashConversion`), so the maps themselves are compared.
//!
//! For each `<Spec>` both sides build the items as `ItemsTab:Load` does (`Item("")`, variants,
//! `ParseRaw`, `ModRange`, `BuildModList`), then `new("PassiveSpec")` + `Load` + `PostLoad`
//! (= `BuildClusterJewelGraphs`), and record: the subgraph ids, each subgraph's parent socket,
//! entrance, group (x, y, orbit flags) and node list; every subgraph node's type, name, stats,
//! orbit / index, position (x, y, angle as `%.17g`), icon, flags, `linked` ids, `modKey`,
//! `mods` and `modList` (formatMod + source); the parent socket's `linked`; the allocated set
//! and order, the alloc flags, `allocExtendedNodes`, `allocSubgraphNodes`, `jewels`, the
//! cluster node ids in `spec.nodes` and the legacy-conversion fields. Then three rounds of
//! "allocate every subgraph node, rebuild" (which descends into nested sockets), and the undo
//! round trip (`RestoreUndoState(CreateUndoState())` on a new spec, then
//! `BuildClusterJewelGraphs`, as TreeTab / CompareTab do) including the window title class text.
//!
//! Function probes (after all builds, since they mutate shared tree nodes): every
//! `TranslateClusterOrbitIndex` pair of {6, 12, 16, 40} × index -1..16 and 1.5; on a sample of
//! nodes of every tree version used, `ReplaceNode` with legion and tattoo nodes and back,
//! `NodeAdditionOrReplacementFromString` sequences (both modes, multi-line and unparseable
//! lines), `NodeInKeystoneRadius` for every jewel radius and `ReconnectNodeToClassStart` on
//! every node.
//!
//! Orders that follow `pairs(tree.nodes)` / `pairs(tree.sockets)` are random per process in PoB
//! for 3.10+ trees (CCR-0024 §5): for those builds the order-carrying records (`alloco`, `ext`,
//! `jewels`) are compared as sets (asserted) and in order (reported).
//!
//! Self-skips without a PoB checkout, `data/dump` or the corpus (`POB_ORACLE_REQUIRE=1` makes
//! that a failure). `PSC_BUILDS=substr,...` limits the corpus builds, `PSC_SYNTH=0` skips the
//! synthetic builds.
#![allow(non_snake_case, clippy::too_many_lines)]

use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;

use pob_calc::classes::item::new_Item;
use pob_calc::classes::passive_spec::{
    main_LoadTree, main_tree_insert, new_PassiveSpec, set_spec_stubs, SpecStubs,
};
use pob_calc::classes::passive_spec_cluster::set_window_title_subtext_hook;
use pob_calc::classes::passive_tree::{new_passive_tree_with_node_order, setJewelRadiiGlobally};
use pob_calc::classes::{Node, NodeRef, PassiveSpecRef};
use pob_calc::lua_table::{Key, LStr, Table, Val};
use pob_calc::model::{Build, BuildRef, XmlChild, XmlNode};
use pob_gamedata::{data, Name};
use pob_lua::{lfmt, tostring_num};
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

const LIB: &str = r#"
ConPrintf = function() end
if launch then launch.ShowErrMsg = function() end end
if not common.classes.PassiveSpec then LoadModule("Classes/PassiveSpec") end
__psc = { }
local PSC = common.classes.PassiveSpec
-- PS-B is a no-op on both sides (CCR-0052 §12): the counters it initialises (the port's fields start at 0)
PSC.BuildAllDependsAndPaths = function(self)
    self.allocatedMasteryCount = self.allocatedMasteryCount or 0
    self.allocatedMasteryTypeCount = self.allocatedMasteryTypeCount or 0
    self.allocatedMasteryTypes = self.allocatedMasteryTypes or { }
end
if main then main.SetWindowTitleSubtext = function(_, s) __psc.title = s end
else main = { SetWindowTitleSubtext = function(_, s) __psc.title = s end } end
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
local build = { itemsTab = { items = { } }, treeTab = { viewer = { } }, data = data, buildName = "B" }
function __psc.load(xmlText)
    local dbXML = common.xml.ParseXML(xmlText)
    __psc.specNodes = { }
    __psc.itemsNode = nil
    local out = { }
    for _, top in ipairs(dbXML) do
        if type(top) == "table" and top.elem == "PathOfBuilding" then
            for _, sec in ipairs(top) do
                if type(sec) == "table" and sec.elem == "Tree" then
                    for _, sp in ipairs(sec) do
                        if type(sp) == "table" and sp.elem == "Spec" then
                            __psc.specNodes[#__psc.specNodes + 1] = sp
                            ser(sp, out)
                        end
                    end
                elseif type(sec) == "table" and sec.elem == "Items" then
                    __psc.itemsNode = sec
                    ser(sec, out)
                end
            end
        end
    end
    -- ItemsTab:Load (ItemsTab.lua 1221-1269)
    build.itemsTab.items = { }
    for _, node in ipairs(__psc.itemsNode or { }) do
        if type(node) == "table" and node.elem == "Item" then
            local item = new("Item"):Item("")
            item.id = tonumber(node.attrib.id)
            item.variant = tonumber(node.attrib.variant)
            for _, k in ipairs({ "variantAlt", "variantAlt2", "variantAlt3", "variantAlt4", "variantAlt5" }) do
                if node.attrib[k] then
                    item["has" .. k:gsub("^v", "V")] = true
                    item[k] = tonumber(node.attrib[k])
                end
            end
            for _, child in ipairs(node) do
                if type(child) == "string" then
                    item:ParseRaw(child)
                elseif child.elem == "ModRange" then
                    local id = tonumber(child.attrib.id) or 0
                    local range = tonumber(child.attrib.range) or 1
                    for _, list in ipairs{item.buffModLines, item.enchantModLines, item.scourgeModLines, item.implicitModLines, item.explicitModLines, item.crucibleModLines} do
                        if id <= #list then
                            list[id].range = range
                            break
                        end
                        id = id - #list
                    end
                end
            end
            if item.base then
                item:BuildModList()
                build.itemsTab.items[item.id] = item
            end
        end
    end
    return table.concat(out, "\n")
end
local function sortedKeys(t) local p = { } for k in pairs(t) do p[#p + 1] = k end table.sort(p) return p end
local function joinNums(list) local p = { } for i, v in ipairs(list) do p[i] = tostring(v) end return table.concat(p, ",") end
local function keysInOrder(t) local p = { } for k in pairs(t) do p[#p + 1] = tostring(k) end return table.concat(p, ",") end
local function kvOrder(t) local p = { } for k, v in pairs(t) do p[#p + 1] = tostring(k) .. "=" .. tostring(v) end return table.concat(p, ",") end
local function kvSorted(t) local p = { } for _, k in ipairs(sortedKeys(t)) do p[#p + 1] = tostring(k) .. "=" .. tostring(t[k]) end return table.concat(p, ",") end
local function f17(x) if x == nil then return "nil" end return string.format("%.17g", x) end
local function modsStr(ml)
    local p = { }
    for i, m in ipairs(ml or { }) do p[i] = modLib.formatMod(m) .. "@" .. tostring(m.source) end
    return table.concat(p, ";")
end
local function linkedStr(n) local p = { } for i, o in ipairs(n.linked or { }) do p[i] = tostring(o.id) end return table.concat(p, ",") end
local function modsEntries(n)
    local p = { }
    for i, e in ipairs(n.mods or { }) do p[i] = (e.list and tostring(#e.list) or "nil") .. ":" .. tostring(e.extra) end
    return table.concat(p, ",")
end
local function nodeRec(pre, n, emit)
    emit(pre .. "f", table.concat({ tostring(n.type), tostring(n.dn), tostring(n.o), tostring(n.oidx), f17(n.x), f17(n.y), f17(n.angle),
        tostring(n.icon), tostring(n.expansionSkill == true), tostring(n.alloc == true) }, "|"))
    emit(pre .. "sd", table.concat(n.sd or { }, "/"))
    emit(pre .. "linked", linkedStr(n))
    emit(pre .. "modKey", tostring(n.modKey))
    emit(pre .. "mods", modsEntries(n))
    emit(pre .. "modList", modsStr(n.modList))
    if n.expansionJewel then
        local ej = n.expansionJewel
        emit(pre .. "ej", tostring(ej.size) .. "," .. tostring(ej.index) .. "," .. tostring(ej.proxy) .. "," .. tostring(ej.parent))
    end
end
local function state(spec, pre, emit)
    local ids = sortedKeys(spec.subGraphs)
    emit(pre .. "sg", joinNums(ids))
    for _, id in ipairs(ids) do
        local sg = spec.subGraphs[id]
        local p = pre .. "sg/" .. id .. "/"
        emit(p .. "parent", tostring(sg.parentSocket.id))
        emit(p .. "entrance", tostring(sg.entranceNode and sg.entranceNode.id))
        emit(p .. "group", f17(sg.group.x) .. "," .. f17(sg.group.y) .. ",oo=" .. joinNums(sortedKeys(sg.group.oo)))
        local ns = { }
        for i, n in ipairs(sg.nodes) do ns[i] = tostring(n.id) end
        emit(p .. "nodes", table.concat(ns, ","))
        for _, n in ipairs(sg.nodes) do nodeRec(p .. "n/" .. tostring(n.id) .. "/", n, emit) end
        emit(p .. "parentLinked", linkedStr(sg.parentSocket))
    end
    emit(pre .. "allocset", joinNums(sortedKeys(spec.allocNodes)))
    emit(pre .. "alloco", keysInOrder(spec.allocNodes))
    local flags, cl = { }, { }
    for id, n in pairs(spec.nodes) do
        if n.alloc then flags[#flags + 1] = id end
        if id >= 65536 then cl[#cl + 1] = id end
    end
    table.sort(flags)
    table.sort(cl)
    emit(pre .. "allocflag", joinNums(flags))
    emit(pre .. "clusterNodes", joinNums(cl))
    emit(pre .. "ext", joinNums(spec.allocExtendedNodes))
    emit(pre .. "sub", joinNums(spec.allocSubgraphNodes))
    emit(pre .. "jewels", kvOrder(spec.jewels))
    emit(pre .. "jewelset", kvSorted(spec.jewels))
    emit(pre .. "misc", table.concat({ tostring(spec.clusterHashFormatVersion), tostring(spec.checkNodeLinks), tostring(spec.ignoreAllocatingSubgraph),
        tostring(spec.legacyClusterNodeMap == nil), tostring(spec.legacyClusterNodeMapReverse == nil) }, "|"))
end
local function allocAll(spec)
    for _, id in ipairs(sortedKeys(spec.subGraphs)) do
        for _, n in ipairs(spec.subGraphs[id].nodes) do
            if not n.alloc then
                n.alloc = true
                spec.allocNodes[n.id] = n
            end
        end
    end
    spec:BuildClusterJewelGraphs()
end
local function newSpec(ver) return new("PassiveSpec"):PassiveSpec(build, ver) end
-- the legacy (v1) conversion step by step, as BuildClusterJewelGraphs runs it on a fresh spec
-- (sockets in id order), so the maps themselves can be compared before End clears them
local function legacyPhase(node, ver, emit)
    local s = newSpec(ver)
    s:Load(node, "corpus")
    if (s.clusterHashFormatVersion or 2) >= 2 then return end
    emit("legacy/active", tostring(s:BeginLegacyClusterHashConversion()))
    for _, nodeId in ipairs(sortedKeys(s.tree.sockets)) do
        local n = s.tree.nodes[nodeId]
        local jewel = s:GetSocketedJewel(nodeId)
        if n and s.allocNodes[n.id] and n.expansionJewel and n.expansionJewel.size == 2 and jewel and jewel.jewelData.clusterJewelValid then
            s:BuildSubgraph(jewel, s.nodes[nodeId], nil, nil, { }, { })
        end
    end
    emit("legacy/map", kvOrder(s.legacyClusterNodeMap))
    emit("legacy/rev", kvSorted(s.legacyClusterNodeMapReverse))
    emit("legacy/presub", joinNums(s.allocSubgraphNodes))
    s:ApplyLegacyClusterNodeRemap()
    emit("legacy/sub", joinNums(s.allocSubgraphNodes))
    emit("legacy/alloc", joinNums(sortedKeys(s.allocNodes)))
    emit("legacy/jewels", kvSorted(s.jewels))
    s:EndLegacyClusterHashConversion()
    emit("legacy/misc", tostring(s.clusterHashFormatVersion) .. "|" .. tostring(s.legacyClusterNodeMap == nil) .. "|" .. tostring(s.legacyClusterNodeMapReverse == nil))
end
function __psc.dump(i)
    build.spec = nil
    local out = { }
    local function emit(k, v) out[#out + 1] = k .. "\t" .. esc(v) end
    local node = __psc.specNodes[i]
    local ver = node.attrib.treeVersion or defaultTreeVersion
    legacyPhase(node, ver, emit)
    local spec = newSpec(ver)
    spec:Load(node, "corpus")
    emit("preload/sub", joinNums(spec.allocSubgraphNodes))
    emit("preload/jewels", kvSorted(spec.jewels))
    spec:PostLoad()
    state(spec, "load/", emit)
    for r = 1, 3 do
        allocAll(spec)
        state(spec, "a" .. r .. "/", emit)
    end
    local st = spec:CreateUndoState()
    emit("undo/state", table.concat({ tostring(st.classId), tostring(st.ascendClassId), tostring(st.secondaryAscendClassId), tostring(st.treeVersion) }, "|"))
    emit("undo/hashList", joinNums(st.hashList))
    emit("undo/hashListSet", joinNums((function() local t = { } for i, v in ipairs(st.hashList) do t[i] = v end table.sort(t) return t end)()))
    emit("undo/masterySet", kvSorted(st.masteryEffects))
    local ov = { }
    for _, k in ipairs(sortedKeys(st.hashOverrides)) do ov[#ov + 1] = k .. ":" .. tostring(st.hashOverrides[k].dn) end
    emit("undo/overrides", table.concat(ov, ","))
    local s2 = newSpec(ver)
    __psc.title = nil
    s2:RestoreUndoState(st)
    emit("undo/title", tostring(__psc.title))
    s2:BuildClusterJewelGraphs()
    state(s2, "undo/", emit)
    return table.concat(out, "\n")
end
function __psc.dumpSafe(i)
    local ok, r = pcall(__psc.dump, i)
    if ok then return r end
    return "ERR\t" .. esc(r)
end
function __psc.translate()
    local out = { }
    local spec = __psc.anySpec or newSpec(defaultTreeVersion)
    __psc.anySpec = spec
    for _, a in ipairs({ 6, 12, 16, 40 }) do
        for _, b in ipairs({ 6, 12, 16, 40 }) do
            for _, x in ipairs({ -1, 0, 1, 1.5, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16 }) do
                out[#out + 1] = a .. ":" .. b .. ":" .. tostring(x) .. "=" .. tostring(spec:TranslateClusterOrbitIndex(x, a, b))
            end
        end
    end
    return table.concat(out, "\n")
end
local ADDS = {
    { " \n+5 to Devotion", false },
    { "+10 to Strength", true },
    { "10% increased Ward\n+12 to Dexterity", false },
    { "Totally unparseable line", false },
    { "Minions deal 10% increased Damage", false },
    { "+15 to maximum Life\n \n5% increased maximum Mana", true },
}
local function fields(n)
    return table.concat({ tostring(n.dn), table.concat(n.sd or { }, "/"), tostring(n.name), tostring(n.modKey), modsStr(n.modList), tostring(n.isTattoo == true),
        tostring(n.overrideType), n.keystoneMod and modLib.formatMod(n.keystoneMod) or "nil", tostring(n.icon), table.concat(n.reminderText or { }, "/"),
        tostring(n.activeEffectImage), modsEntries(n) }, "|")
end
function __psc.probe(ver, keystoneList)
    local out = { }
    local function emit(k, v) out[#out + 1] = k .. "\t" .. esc(v) end
    local spec = newSpec(ver)
    local tree = spec.tree
    local ids = sortedKeys(spec.nodes)
    local legion = tree.legion and tree.legion.nodes or { }
    local tattooNames = sortedKeys(tree.tattoo and tree.tattoo.nodes or { })
    local sample = { }
    for i = 1, #ids, 37 do sample[#sample + 1] = ids[i] end
    for si, id in ipairs(sample) do
        local node = spec.nodes[id]
        local p = "rep/" .. id .. "/"
        if #legion > 0 then
            local ln = legion[(si % #legion) + 1]
            emit(p .. "legion", tostring((spec:ReplaceNode(node, ln))) .. "|" .. fields(node))
        end
        emit(p .. "back", tostring((spec:ReplaceNode(node, tree.nodes[id]))) .. "|" .. fields(node))
        emit(p .. "same", tostring((spec:ReplaceNode(node, tree.nodes[id]))) .. "|" .. fields(node))
        if #tattooNames > 0 then
            local tn = tree.tattoo.nodes[tattooNames[(si % #tattooNames) + 1]]
            emit(p .. "tattoo", tostring((spec:ReplaceNode(node, tn))) .. "|" .. fields(node))
            emit(p .. "back2", tostring((spec:ReplaceNode(node, tree.nodes[id]))) .. "|" .. fields(node))
        end
        for ai, a in ipairs(ADDS) do
            spec:NodeAdditionOrReplacementFromString(node, a[1], a[2])
            emit(p .. "add" .. ai, fields(node))
        end
        spec:ReplaceNode(node, tree.nodes[id])
        emit(p .. "restored", fields(node))
    end
    local names = { }
    for _, k in ipairs(keystoneList) do names[k] = true end
    for ri = 1, #data.jewelRadius do
        local hit = { }
        for _, id in ipairs(ids) do
            if spec:NodeInKeystoneRadius(names, id, ri) then hit[#hit + 1] = id end
        end
        emit("ks/" .. ri, joinNums(hit))
    end
    local rc = { }
    for _, id in ipairs(ids) do
        local node = spec.nodes[id]
        local before = #node.modList
        spec:ReconnectNodeToClassStart(node)
        if #node.modList ~= before then rc[#rc + 1] = id .. ":" .. modsStr(node.modList) end
    end
    emit("reconnect", table.concat(rc, ","))
    return table.concat(out, "\n")
end
"#;

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

/// The serialised XML of `__psc.load` → nodes.
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

// ---- Rust side ------------------------------------------------------------------------------

fn tostr_val(v: &Val) -> String {
    match v {
        Val::Nil => "nil".to_owned(),
        Val::Bool(b) => b.to_string(),
        Val::Num(x) => tostring_num(*x),
        Val::Str(s) => s.to_string(),
        other => format!("<{}>", other.lua_type()),
    }
}

fn tostr_num_opt(x: f64) -> String {
    if x.is_nan() {
        "nil".to_owned()
    } else {
        tostring_num(x)
    }
}

fn f17(x: f64) -> String {
    if x.is_nan() {
        "nil".to_owned()
    } else {
        lfmt!("%.17g", x)
    }
}

fn join_nums<I: IntoIterator<Item = f64>>(it: I) -> String {
    it.into_iter()
        .map(tostring_num)
        .collect::<Vec<_>>()
        .join(",")
}

fn sorted_int_keys(t: &Table) -> Vec<i64> {
    let mut v: Vec<i64> = t
        .pairs()
        .filter_map(|(k, _)| match k {
            Key::I(i) => Some(i),
            _ => None,
        })
        .collect();
    v.sort_unstable();
    v
}

fn mods_str(node: &Node) -> String {
    node.modList
        .get()
        .ipairs()
        .iter()
        .map(|m| {
            format!(
                "{}@{}",
                pob_gamedata::mod_tools::formatMod(m),
                m.source.as_deref().unwrap_or("nil")
            )
        })
        .collect::<Vec<_>>()
        .join(";")
}

fn linked_str(n: &Node) -> String {
    n.linked
        .get()
        .snapshot()
        .iter()
        .map(|o| tostr_num_opt(o.id))
        .collect::<Vec<_>>()
        .join(",")
}

fn mods_entries(n: &Node) -> String {
    n.mods
        .get()
        .snapshot()
        .iter()
        .map(|e| {
            let l = e
                .table(Name::from("list"))
                .map_or_else(|| "nil".to_owned(), |t| t.len().to_string());
            format!("{l}:{}", tostr_val(&e.get(Name::from("extra"))))
        })
        .collect::<Vec<_>>()
        .join(",")
}

fn sd_str(n: &Node) -> String {
    n.sd.get()
        .snapshot()
        .iter()
        .map(|s| s.to_string())
        .collect::<Vec<_>>()
        .join("/")
}

fn node_rec(pre: &str, n: &Node, r: &mut Rec) {
    r.emit(
        format!("{pre}f"),
        [
            n.r#type.get().to_string(),
            n.dn.get().to_string(),
            tostr_num_opt(n.o.get()),
            tostr_num_opt(n.oidx.get()),
            f17(n.x.get()),
            f17(n.y.get()),
            f17(n.angle.get()),
            tostr_val(&n.extra.get(Name::from("icon"))),
            (n.extra
                .get(Name::from("expansionSkill"))
                .raw_eq(&Val::Bool(true)))
            .to_string(),
            n.alloc.get().to_string(),
        ]
        .join("|"),
    );
    r.emit(format!("{pre}sd"), sd_str(n));
    r.emit(format!("{pre}linked"), linked_str(n));
    r.emit(format!("{pre}modKey"), &*n.modKey.get());
    r.emit(format!("{pre}mods"), mods_entries(n));
    r.emit(format!("{pre}modList"), mods_str(n));
    if let Some(ej) = n.expansionJewel.get() {
        r.emit(
            format!("{pre}ej"),
            format!(
                "{},{},{},{}",
                tostr_val(&ej.get(Name::from("size"))),
                tostr_val(&ej.get(Name::from("index"))),
                tostr_val(&ej.get(Name::from("proxy"))),
                tostr_val(&ej.get(Name::from("parent")))
            ),
        );
    }
}

fn obj_node(v: &Val) -> NodeRef {
    v.obj::<Node>().expect("node value")
}

fn state(spec: &PassiveSpecRef, pre: &str, r: &mut Rec) {
    let ids = sorted_int_keys(&spec.subGraphs);
    r.emit(format!("{pre}sg"), join_nums(ids.iter().map(|&x| x as f64)));
    for id in ids {
        let sg = spec.subGraphs.table(id).unwrap();
        let p = format!("{pre}sg/{id}/");
        let parent = obj_node(&sg.get(Name::from("parentSocket")));
        r.emit(format!("{p}parent"), tostr_num_opt(parent.id));
        r.emit(
            format!("{p}entrance"),
            sg.get(Name::from("entranceNode"))
                .obj::<Node>()
                .map_or_else(|| "nil".to_owned(), |n| tostr_num_opt(n.id)),
        );
        let g = sg.table(Name::from("group")).unwrap();
        let oo = g.table(Name::from("oo")).unwrap();
        r.emit(
            format!("{p}group"),
            format!(
                "{},{},oo={}",
                f17(g.get(Name::from("x")).opt_num().unwrap_or(f64::NAN)),
                f17(g.get(Name::from("y")).opt_num().unwrap_or(f64::NAN)),
                join_nums(sorted_int_keys(&oo).into_iter().map(|x| x as f64))
            ),
        );
        let nodes: Vec<NodeRef> = sg
            .table(Name::from("nodes"))
            .unwrap()
            .ipairs()
            .into_iter()
            .map(|(_, v)| obj_node(&v))
            .collect();
        r.emit(
            format!("{p}nodes"),
            nodes
                .iter()
                .map(|n| tostr_num_opt(n.id))
                .collect::<Vec<_>>()
                .join(","),
        );
        for n in &nodes {
            node_rec(&format!("{p}n/{}/", tostr_num_opt(n.id)), n, r);
        }
        r.emit(format!("{p}parentLinked"), linked_str(&parent));
    }
    let mut alloc: Vec<i64> = spec.allocNodes.with(|m| m.keys());
    r.emit(
        format!("{pre}alloco"),
        join_nums(alloc.iter().map(|&x| x as f64)),
    );
    alloc.sort_unstable();
    r.emit(
        format!("{pre}allocset"),
        join_nums(alloc.iter().map(|&x| x as f64)),
    );
    let pairs = spec.nodes.with(|m| m.pairs());
    let mut flags: Vec<i64> = pairs
        .iter()
        .filter(|(_, n)| n.alloc.get())
        .map(|(k, _)| *k)
        .collect();
    flags.sort_unstable();
    let mut cl: Vec<i64> = pairs
        .iter()
        .filter(|(k, _)| *k >= 65536)
        .map(|(k, _)| *k)
        .collect();
    cl.sort_unstable();
    r.emit(
        format!("{pre}allocflag"),
        join_nums(flags.into_iter().map(|x| x as f64)),
    );
    r.emit(
        format!("{pre}clusterNodes"),
        join_nums(cl.into_iter().map(|x| x as f64)),
    );
    r.emit(
        format!("{pre}ext"),
        join_nums(spec.allocExtendedNodes.snapshot()),
    );
    r.emit(
        format!("{pre}sub"),
        join_nums(spec.allocSubgraphNodes.snapshot()),
    );
    let jw = spec.jewels.with(|m| m.pairs());
    r.emit(
        format!("{pre}jewels"),
        jw.iter()
            .map(|(k, v)| format!("{k}={}", tostring_num(*v)))
            .collect::<Vec<_>>()
            .join(","),
    );
    let mut jws = jw.clone();
    jws.sort_by_key(|(k, _)| *k);
    r.emit(
        format!("{pre}jewelset"),
        jws.iter()
            .map(|(k, v)| format!("{k}={}", tostring_num(*v)))
            .collect::<Vec<_>>()
            .join(","),
    );
    r.emit(
        format!("{pre}misc"),
        [
            tostr_val(&spec.extra.get(Name::from("clusterHashFormatVersion"))),
            tostr_val(&spec.extra.get(Name::from("checkNodeLinks"))),
            tostr_val(&spec.extra.get(Name::from("ignoreAllocatingSubgraph"))),
            spec.extra
                .get(Name::from("legacyClusterNodeMap"))
                .is_nil()
                .to_string(),
            spec.extra
                .get(Name::from("legacyClusterNodeMapReverse"))
                .is_nil()
                .to_string(),
        ]
        .join("|"),
    );
}

fn alloc_all(spec: &PassiveSpecRef) {
    for id in sorted_int_keys(&spec.subGraphs) {
        let sg = spec.subGraphs.table(id).unwrap();
        for (_, v) in sg.table(Name::from("nodes")).unwrap().ipairs() {
            let n = obj_node(&v);
            if !n.alloc.get() {
                n.alloc.set(true);
                spec.allocNodes
                    .with_mut(|m| m.set(n.id as i64, Some(n.clone())));
            }
        }
    }
    spec.BuildClusterJewelGraphs();
}

/// `ItemsTab:Load` (ItemsTab.lua 1221-1269) of the `<Items>` node.
fn load_items(build: &BuildRef, items: Option<&XmlNode>) {
    build.itemsTab.items.with_mut(indexmap::IndexMap::clear);
    let Some(items) = items else { return };
    for c in &items.children {
        let XmlChild::Node(node) = c else { continue };
        if &*node.elem != "Item" {
            continue;
        }
        let item = new_Item(Some(""), None, false);
        let attr = |k: &str| node.attrib.get(k).and_then(|s| pob_lua::tonumber(s));
        item.id.set(attr("id"));
        item.variant.set(attr("variant"));
        for (k, cell) in [
            ("variantAlt", &item.variantAlt),
            ("variantAlt2", &item.variantAlt2),
            ("variantAlt3", &item.variantAlt3),
            ("variantAlt4", &item.variantAlt4),
            ("variantAlt5", &item.variantAlt5),
        ] {
            if node.attrib.contains_key(k) {
                let has = format!("has{}{}", &k[..1].to_uppercase(), &k[1..]);
                item.extra.set(Name::from(has.as_str()), true);
                cell.set(attr(k));
            }
        }
        for child in &node.children {
            match child {
                XmlChild::Text(t) => item.ParseRaw(t, None, false),
                XmlChild::Node(c) if &*c.elem == "ModRange" => {
                    let mut id = c
                        .attrib
                        .get("id")
                        .and_then(|s| pob_lua::tonumber(s))
                        .unwrap_or(0.0);
                    let range = c
                        .attrib
                        .get("range")
                        .and_then(|s| pob_lua::tonumber(s))
                        .unwrap_or(1.0);
                    for list in [
                        item.buffModLines.get(),
                        item.enchantModLines.get(),
                        item.scourgeModLines.get(),
                        item.implicitModLines.get(),
                        item.explicitModLines.get(),
                        item.crucibleModLines.get(),
                    ] {
                        if id <= list.len() as f64 {
                            list.get(id as usize)
                                .expect("mod line")
                                .set(Name::from("range"), range);
                            break;
                        }
                        id -= list.len() as f64;
                    }
                }
                XmlChild::Node(_) => {}
            }
        }
        if item.base.get().is_some() {
            item.BuildModList();
            let id = item.id.get().expect("item id") as u32;
            build.itemsTab.items.with_mut(|m| {
                m.insert(id, item.clone());
            });
        }
    }
}

fn tree_version_of(node: &XmlNode) -> Name {
    node.attrib.get("treeVersion").map_or_else(
        || data().gameVersions.defaultTreeVersion,
        |s| Name::from(&**s),
    )
}

thread_local! {
    static TITLE: std::cell::RefCell<Option<String>> = const { std::cell::RefCell::new(None) };
}

fn kv_sorted_table(t: &Table) -> String {
    sorted_int_keys(t)
        .into_iter()
        .map(|k| format!("{k}={}", tostr_val(&t.get(k))))
        .collect::<Vec<_>>()
        .join(",")
}

/// The legacy conversion step by step (as `legacyPhase` in the oracle).
fn legacy_phase(build: &BuildRef, xml: &XmlNode, r: &mut Rec) {
    let ver = tree_version_of(xml);
    let s = new_PassiveSpec(build, ver, false);
    s.Load(xml, "corpus");
    if s.extra.num_or(Name::from("clusterHashFormatVersion"), 2.0) >= 2.0 {
        return;
    }
    r.emit(
        "legacy/active",
        s.BeginLegacyClusterHashConversion_ret().to_string(),
    );
    let tree = s.tree.get();
    let mut ids = tree.sockets.keys();
    ids.sort_unstable();
    let (ig, inodes) = (Table::new(), Table::new());
    for nodeId in ids {
        let n = tree.nodes.get(nodeId);
        let (jewel, _) = s.GetSocketedJewel(nodeId as f64);
        let Some(n) = n else { continue };
        let Some(jewel) = jewel else { continue };
        let ok = s.allocNodes.with(|m| m.contains_key(n.id as i64))
            && n.expansionJewel
                .get()
                .is_some_and(|e| e.get(Name::from("size")).raw_eq(&Val::Num(2.0)))
            && jewel
                .jewelData
                .get()
                .unwrap()
                .truthy(Name::from("clusterJewelValid"));
        if ok {
            let parent = s.nodes.with(|m| m.get(nodeId)).unwrap();
            s.BuildSubgraph(&jewel, &parent, None, None, Some(&inodes), Some(&ig));
        }
    }
    let map = s
        .extra
        .obj::<std::cell::RefCell<pob_lua::LjIntMap<f64>>>(Name::from("legacyClusterNodeMap"))
        .expect("legacy map");
    r.emit(
        "legacy/map",
        map.borrow()
            .pairs()
            .iter()
            .map(|(k, v)| format!("{k}={}", tostring_num(*v)))
            .collect::<Vec<_>>()
            .join(","),
    );
    r.emit(
        "legacy/rev",
        kv_sorted_table(
            &s.extra
                .table(Name::from("legacyClusterNodeMapReverse"))
                .expect("reverse map"),
        ),
    );
    r.emit("legacy/presub", join_nums(s.allocSubgraphNodes.snapshot()));
    s.ApplyLegacyClusterNodeRemap();
    r.emit("legacy/sub", join_nums(s.allocSubgraphNodes.snapshot()));
    let mut alloc: Vec<i64> = s.allocNodes.with(|m| m.keys());
    alloc.sort_unstable();
    r.emit(
        "legacy/alloc",
        join_nums(alloc.into_iter().map(|x| x as f64)),
    );
    let mut jws = s.jewels.with(|m| m.pairs());
    jws.sort_by_key(|(k, _)| *k);
    r.emit(
        "legacy/jewels",
        jws.iter()
            .map(|(k, v)| format!("{k}={}", tostring_num(*v)))
            .collect::<Vec<_>>()
            .join(","),
    );
    s.EndLegacyClusterHashConversion();
    r.emit(
        "legacy/misc",
        format!(
            "{}|{}|{}",
            tostr_val(&s.extra.get(Name::from("clusterHashFormatVersion"))),
            s.extra.get(Name::from("legacyClusterNodeMap")).is_nil(),
            s.extra
                .get(Name::from("legacyClusterNodeMapReverse"))
                .is_nil()
        ),
    );
}

fn rust_dump(build: &BuildRef, xml: &XmlNode) -> BTreeMap<String, String> {
    let mut r = Rec(BTreeMap::new());
    legacy_phase(build, xml, &mut r);
    let ver = tree_version_of(xml);
    let spec = new_PassiveSpec(build, ver, false);
    spec.Load(xml, "corpus");
    r.emit("preload/sub", join_nums(spec.allocSubgraphNodes.snapshot()));
    {
        let mut jws = spec.jewels.with(|m| m.pairs());
        jws.sort_by_key(|(k, _)| *k);
        r.emit(
            "preload/jewels",
            jws.iter()
                .map(|(k, v)| format!("{k}={}", tostring_num(*v)))
                .collect::<Vec<_>>()
                .join(","),
        );
    }
    spec.PostLoad();
    state(&spec, "load/", &mut r);
    for round in 1..=3 {
        alloc_all(&spec);
        state(&spec, &format!("a{round}/"), &mut r);
    }
    let st = spec.CreateUndoState();
    r.emit(
        "undo/state",
        [
            tostr_val(&st.get(Name::from("classId"))),
            tostr_val(&st.get(Name::from("ascendClassId"))),
            tostr_val(&st.get(Name::from("secondaryAscendClassId"))),
            tostr_val(&st.get(Name::from("treeVersion"))),
        ]
        .join("|"),
    );
    let hl: Vec<f64> = st
        .table(Name::from("hashList"))
        .unwrap()
        .ipairs()
        .into_iter()
        .map(|(_, v)| v.num())
        .collect();
    r.emit("undo/hashList", join_nums(hl.iter().copied()));
    let mut hls = hl.clone();
    hls.sort_by(f64::total_cmp);
    r.emit("undo/hashListSet", join_nums(hls));
    let me = st.table(Name::from("masteryEffects")).unwrap();
    r.emit(
        "undo/masterySet",
        sorted_int_keys(&me)
            .into_iter()
            .map(|k| format!("{k}={}", tostr_val(&me.get(k))))
            .collect::<Vec<_>>()
            .join(","),
    );
    let ov = st.table(Name::from("hashOverrides")).unwrap();
    r.emit(
        "undo/overrides",
        sorted_int_keys(&ov)
            .into_iter()
            .map(|k| format!("{k}:{}", obj_node(&ov.get(k)).dn.get()))
            .collect::<Vec<_>>()
            .join(","),
    );
    let s2 = new_PassiveSpec(build, ver, false);
    TITLE.with(|t| *t.borrow_mut() = None);
    s2.RestoreUndoState(&st, None);
    // PoB's subtext is `"%s (%s)"` of the build name ("B") and the class text
    let title = TITLE.with(|t| t.borrow().clone());
    r.emit(
        "undo/title",
        title.map_or_else(|| "nil".to_owned(), |c| format!("B ({c})")),
    );
    s2.BuildClusterJewelGraphs();
    state(&s2, "undo/", &mut r);
    r.0
}

fn fields(n: &Node) -> String {
    [
        n.dn.get().to_string(),
        sd_str(n),
        tostr_val(&n.extra.get(Name::from("name"))),
        n.modKey.get().to_string(),
        mods_str(n),
        n.isTattoo.get().to_string(),
        n.overrideType
            .get()
            .map_or_else(|| "nil".to_owned(), |x| x.to_string()),
        n.keystoneMod.get().map_or_else(
            || "nil".to_owned(),
            |m| pob_gamedata::mod_tools::formatMod(&m),
        ),
        tostr_val(&n.extra.get(Name::from("icon"))),
        match n.extra.get(Name::from("reminderText")) {
            Val::Table(t) => t
                .ipairs()
                .iter()
                .map(|(_, v)| tostr_val(v))
                .collect::<Vec<_>>()
                .join("/"),
            _ => String::new(),
        },
        tostr_val(&n.extra.get(Name::from("activeEffectImage"))),
        mods_entries(n),
    ]
    .join("|")
}

const ADDS: [(&str, bool); 6] = [
    (" \n+5 to Devotion", false),
    ("+10 to Strength", true),
    ("10% increased Ward\n+12 to Dexterity", false),
    ("Totally unparseable line", false),
    ("Minions deal 10% increased Damage", false),
    ("+15 to maximum Life\n \n5% increased maximum Mana", true),
];

fn rust_probe(build: &BuildRef, ver: Name, keystones: &[&str]) -> BTreeMap<String, String> {
    let mut r = Rec(BTreeMap::new());
    let spec = new_PassiveSpec(build, ver, false);
    let tree = spec.tree.get();
    let mut ids: Vec<i64> = spec.nodes.with(|m| m.keys());
    ids.sort_unstable();
    let legion: Vec<NodeRef> = tree
        .legion
        .get()
        .and_then(|l| l.table(Name::from("nodes")))
        .map(|t| t.ipairs().into_iter().map(|(_, v)| obj_node(&v)).collect())
        .unwrap_or_default();
    let tattoo = tree
        .tattoo
        .get()
        .and_then(|t| t.table(Name::from("nodes")))
        .unwrap_or_default();
    let mut tattooNames: Vec<String> = tattoo
        .pairs()
        .filter_map(|(k, _)| k.name().map(|n| n.to_string()))
        .collect();
    tattooNames.sort();
    let sample: Vec<i64> = ids.iter().step_by(37).copied().collect();
    for (si0, &id) in sample.iter().enumerate() {
        let si = si0 + 1;
        let node = spec.nodes.with(|m| m.get(id)).unwrap();
        let treeNode = tree.nodes.get(id).unwrap();
        let p = format!("rep/{id}/");
        let ret = |b: bool| if b { "1".to_owned() } else { "nil".to_owned() };
        if !legion.is_empty() {
            let ln = &legion[si % legion.len()];
            let b = spec.ReplaceNode(&node, ln);
            r.emit(
                format!("{p}legion"),
                format!("{}|{}", ret(b), fields(&node)),
            );
        }
        let b = spec.ReplaceNode(&node, &treeNode);
        r.emit(format!("{p}back"), format!("{}|{}", ret(b), fields(&node)));
        let b = spec.ReplaceNode(&node, &treeNode);
        r.emit(format!("{p}same"), format!("{}|{}", ret(b), fields(&node)));
        if !tattooNames.is_empty() {
            let tn = obj_node(&tattoo.get(tattooNames[si % tattooNames.len()].as_str()));
            let b = spec.ReplaceNode(&node, &tn);
            r.emit(
                format!("{p}tattoo"),
                format!("{}|{}", ret(b), fields(&node)),
            );
            let b = spec.ReplaceNode(&node, &treeNode);
            r.emit(format!("{p}back2"), format!("{}|{}", ret(b), fields(&node)));
        }
        for (ai, (s, rep)) in ADDS.iter().enumerate() {
            spec.NodeAdditionOrReplacementFromString(&node, s, *rep);
            r.emit(format!("{p}add{}", ai + 1), fields(&node));
        }
        spec.ReplaceNode(&node, &treeNode);
        r.emit(format!("{p}restored"), fields(&node));
    }
    let names = Table::new();
    for k in keystones {
        names.set(*k, true);
    }
    let nradii = pob_gamedata::jewelRadius().jewelRadius.len();
    for ri in 1..=nradii {
        let hit: Vec<f64> = ids
            .iter()
            .filter(|&&id| spec.NodeInKeystoneRadius(&names, id as f64, ri as f64))
            .map(|&id| id as f64)
            .collect();
        r.emit(format!("ks/{ri}"), join_nums(hit));
    }
    let mut rc = Vec::new();
    for &id in &ids {
        let node = spec.nodes.with(|m| m.get(id)).unwrap();
        let before = node.modList.get().ipairs().len();
        spec.ReconnectNodeToClassStart(&node);
        if node.modList.get().ipairs().len() != before {
            rc.push(format!("{id}:{}", mods_str(&node)));
        }
    }
    r.emit("reconnect", rc.join(","));
    r.0
}

// ---- synthetic cluster builds ---------------------------------------------------------------

/// One synthetic cluster jewel: base, skill id, node count, socket count (`None`: no line),
/// notables, extra lines.
struct Jewel {
    base: &'static str,
    lines: Vec<String>,
}

fn cluster_item(id: usize, j: &Jewel) -> String {
    let mut s = format!(
        "<Item id=\"{id}\">\nRarity: RARE\nSynth {id}\n{}\nItem Level: 84\n",
        j.base
    );
    let implicits: Vec<&String> = j
        .lines
        .iter()
        .filter(|l| l.starts_with("{crafted}"))
        .collect();
    s.push_str(&format!("Implicits: {}\n", implicits.len()));
    for l in &implicits {
        s.push_str(l);
        s.push('\n');
    }
    for l in j.lines.iter().filter(|l| !l.starts_with("{crafted}")) {
        s.push_str(l);
        s.push('\n');
    }
    s.push_str("</Item>\n");
    s
}

/// The enchant line(s) of a cluster skill.
fn skill_lines(base: &str, skillIdx: usize) -> (String, Vec<String>) {
    let cj = &data().clusterJewels.jewels[&Name::from(base)];
    let (id, sk) = cj.skills.get_index(skillIdx % cj.skills.len()).unwrap();
    (
        id.to_string(),
        sk.enchant
            .iter()
            .map(|e| format!("{{crafted}}{e}"))
            .collect(),
    )
}

fn jewel(
    base: &'static str,
    skillIdx: Option<usize>,
    nodes: usize,
    sockets: usize,
    notables: &[&str],
    extra: &[&str],
) -> Jewel {
    let mut lines = vec![format!("{{crafted}}Adds {nodes} Passive Skills")];
    if sockets == 1 {
        lines.push("{crafted}1 Added Passive Skill is a Jewel Socket".to_owned());
    } else if sockets > 1 {
        lines.push(format!(
            "{{crafted}}{sockets} Added Passive Skills are Jewel Sockets"
        ));
    }
    if let Some(si) = skillIdx {
        let (_, l) = skill_lines(base, si);
        lines.extend(l);
    }
    for n in notables {
        lines.push(format!("1 Added Passive Skill is {n}"));
    }
    for e in extra {
        lines.push((*e).to_owned());
    }
    Jewel { base, lines }
}

/// The large sockets of a tree and, per socket, its proxy group's sub-sockets
/// (`expansionJewel.index` → node id).
fn socket_children(ver: Name, id: i64) -> BTreeMap<i64, i64> {
    let tree = main_LoadTree(ver).unwrap();
    let node = tree.nodes.get(id).unwrap();
    let ej = node.expansionJewel.get().unwrap();
    let proxy = match ej.get(Name::from("proxy")) {
        Val::Num(x) => x as i64,
        Val::Str(s) => s.parse().unwrap(),
        other => panic!("proxy {other:?}"),
    };
    let group = tree.nodes.get(proxy).unwrap().group.get().unwrap();
    let mut out = BTreeMap::new();
    for (_, nid) in group.table(Name::from("n")).unwrap().ipairs() {
        let nid = match nid {
            Val::Num(x) => x as i64,
            Val::Str(s) => s.parse().unwrap(),
            _ => continue,
        };
        let n = tree.nodes.get(nid).unwrap();
        if let Some(e) = n.expansionJewel.get() {
            out.insert(e.get(Name::from("index")).num() as i64, nid);
        }
    }
    out
}

/// How the synthetic jewels sit in a large socket's sockets: Large / Medium / Small in sockets
/// of their own size, a Medium (and Smalls in its sub-sockets) in the large socket, or a Small
/// in the large socket. The last two are the downsized cases whose legacy (v1) ids differ
/// (`BuildLegacyProxyGroup`).
#[derive(Clone, Copy, PartialEq)]
enum Layout {
    Lms,
    Ms,
    S,
}

fn synthetic_builds() -> Vec<(String, String)> {
    let mut out = Vec::new();
    for (ver, layout, variants) in [
        ("3_29", Layout::Lms, 0..4usize),
        ("3_29", Layout::Ms, 0..2),
        ("3_29", Layout::S, 0..1),
        ("3_13", Layout::Lms, 0..1),
        ("3_13", Layout::Ms, 0..1),
        ("3_13", Layout::S, 0..1),
    ] {
        out.extend(synthetic_builds_of(ver, layout, variants));
    }
    out
}

fn synthetic_builds_of(
    verStr: &str,
    layout: Layout,
    variants: std::ops::Range<usize>,
) -> Vec<(String, String)> {
    let ver = Name::from(verStr);
    let tree = main_LoadTree(ver).unwrap();
    let mut large: Vec<i64> = tree
        .sockets
        .pairs()
        .into_iter()
        .filter(|(_, n)| {
            n.expansionJewel
                .get()
                .is_some_and(|e| e.get(Name::from("size")).raw_eq(&Val::Num(2.0)))
        })
        .map(|(k, _)| k)
        .collect();
    large.sort_unstable();
    // notables in the tree's cluster node map, by sort order
    let so = &data().clusterJewels.notableSortOrder;
    let mut notables: Vec<(f64, String)> = so
        .iter()
        .filter(|(n, _)| tree.clusterNodeMap.get(n.as_str()).truthy())
        .map(|(n, v)| (*v, n.to_string()))
        .collect();
    notables.sort_by(|a, b| a.0.total_cmp(&b.0));
    let nb = |i: usize| -> &'static str {
        Box::leak(
            notables[(i * 7) % notables.len()]
                .1
                .clone()
                .into_boxed_str(),
        )
    };
    let ks = data()
        .clusterJewels
        .keystones
        .first()
        .map(|k| k.to_string());

    // (large, [medium per sub-socket], [small per medium sub-socket])
    let larges = [
        jewel("Large Cluster Jewel", Some(0), 8, 2, &[nb(0), nb(1)], &[]),
        jewel(
            "Large Cluster Jewel",
            Some(1),
            12,
            1,
            &[nb(2), nb(3), nb(4)],
            &["Added Small Passive Skills have 25% increased Effect"],
        ),
        jewel(
            "Large Cluster Jewel",
            Some(2),
            10,
            0,
            &[nb(5), nb(6), nb(7)],
            &[],
        ),
        jewel(
            "Large Cluster Jewel",
            Some(3),
            9,
            2,
            &[nb(8)],
            &["Added Small Passive Skills also grant: +5 to Strength"],
        ),
        jewel(
            "Large Cluster Jewel",
            None,
            8,
            0,
            &[],
            &[
                "Adds 3 Jewel Socket Passive Skills",
                "Adds 1 Small Passive Skill which grants nothing",
            ],
        ),
        jewel(
            "Large Cluster Jewel",
            Some(4),
            11,
            2,
            &[nb(9), nb(10), nb(11), nb(12)],
            &[],
        ),
        jewel(
            "Large Cluster Jewel",
            Some(5),
            8,
            1,
            &[nb(13), "Not A Real Notable"],
            &[],
        ),
    ];
    let mediums = [
        jewel(
            "Medium Cluster Jewel",
            Some(0),
            4,
            0,
            &[nb(20), nb(21)],
            &[],
        ),
        jewel("Medium Cluster Jewel", Some(1), 4, 1, &[nb(22)], &[]),
        jewel(
            "Medium Cluster Jewel",
            Some(2),
            5,
            1,
            &[nb(23), nb(24)],
            &[],
        ),
        jewel(
            "Medium Cluster Jewel",
            Some(3),
            6,
            1,
            &[nb(25), nb(26)],
            &["Added Small Passive Skills have 30% increased Effect"],
        ),
        jewel("Medium Cluster Jewel", Some(4), 5, 0, &[nb(27)], &[]),
        jewel(
            "Medium Cluster Jewel",
            Some(5),
            6,
            0,
            &[nb(28), nb(29), nb(30)],
            &[],
        ),
    ];
    let smalls = vec![
        jewel("Small Cluster Jewel", Some(0), 2, 0, &[nb(40)], &[]),
        jewel(
            "Small Cluster Jewel",
            Some(1),
            3,
            0,
            &[nb(41)],
            &["Added Small Passive Skills have 35% increased Effect"],
        ),
        jewel(
            "Small Cluster Jewel",
            Some(2),
            3,
            0,
            &[],
            &["Added Small Passive Skills grant Nothing"],
        ),
        jewel("Small Cluster Jewel", Some(3), 2, 0, &[], &[]),
    ];
    // a keystone cluster (`clusterJewelKeystone`: `o = 0, oidx = 1`, PassiveSpec.lua 2076-2087)
    // in its own build: PoB raises on its position (`orbitAnglesByOrbit[1][2]` is nil), so it
    // would end every build it is in
    let keystoneSmalls: Vec<Jewel> = ks
        .iter()
        .map(|k| {
            let k: &'static str = Box::leak(format!("Adds {k}").into_boxed_str());
            jewel("Small Cluster Jewel", None, 1, 0, &[], &[k])
        })
        .collect();

    let mut out = Vec::new();
    for variant in variants {
        let smalls = if variant == 3 {
            if keystoneSmalls.is_empty() {
                continue;
            }
            &keystoneSmalls
        } else {
            &smalls
        };
        let mut items = String::new();
        let mut sockets = String::new();
        // the legacy twin's sockets: the same jewels, except that a downsized Medium's
        // sub-socket jewels sit at the sockets a v1 save names (`BuildLegacyProxyGroup`'s group)
        let mut legacySockets = String::new();
        let mut nodesAttr: Vec<i64> = Vec::new();
        let mut itemId = 0usize;
        let mut add = |j: &Jewel,
                       socket: i64,
                       legacySocket: i64,
                       items: &mut String,
                       sockets: &mut String,
                       legacySockets: &mut String| {
            itemId += 1;
            items.push_str(&cluster_item(itemId, j));
            sockets.push_str(&format!(
                "<Socket nodeId=\"{socket}\" itemId=\"{itemId}\"/>\n"
            ));
            legacySockets.push_str(&format!(
                "<Socket nodeId=\"{legacySocket}\" itemId=\"{itemId}\"/>\n"
            ));
        };
        for (li, &ls) in large.iter().enumerate() {
            nodesAttr.push(ls);
            if layout == Layout::S {
                add(
                    &smalls[(li + variant) % smalls.len()],
                    ls,
                    ls,
                    &mut items,
                    &mut sockets,
                    &mut legacySockets,
                );
                continue;
            }
            if layout == Layout::Ms {
                add(
                    &mediums[(li + variant) % mediums.len()],
                    ls,
                    ls,
                    &mut items,
                    &mut sockets,
                    &mut legacySockets,
                );
                let children = socket_children(ver, ls);
                // FindClusterSocket(group, 1) or FindClusterSocket(group, 0)
                let legacyParent = children.get(&1).or_else(|| children.get(&0)).copied();
                let legacyChildren = legacyParent
                    .map(|c| socket_children(ver, c))
                    .unwrap_or_default();
                for (si, (idx, &ss)) in children.iter().enumerate() {
                    let leg = legacyChildren.get(idx).copied().unwrap_or(ss);
                    add(
                        &smalls[(li + si + variant) % smalls.len()],
                        ss,
                        leg,
                        &mut items,
                        &mut sockets,
                        &mut legacySockets,
                    );
                }
                continue;
            }
            add(
                &larges[(li + variant) % larges.len()],
                ls,
                ls,
                &mut items,
                &mut sockets,
                &mut legacySockets,
            );
            for (mi, (_, &ms)) in socket_children(ver, ls).iter().enumerate() {
                add(
                    &mediums[(li * 3 + mi + variant) % mediums.len()],
                    ms,
                    ms,
                    &mut items,
                    &mut sockets,
                    &mut legacySockets,
                );
                for (si, (_, &ss)) in socket_children(ver, ms).iter().enumerate() {
                    add(
                        &smalls[(li + mi * 2 + si + variant) % smalls.len()],
                        ss,
                        ss,
                        &mut items,
                        &mut sockets,
                        &mut legacySockets,
                    );
                }
            }
        }
        for legacy in [false, true] {
            let mut nodes = nodesAttr.clone();
            let attr = if legacy {
                // every candidate cluster id of the large sockets (index bits 6-8)
                for li in 0..8i64 {
                    for size in 0..3i64 {
                        for k in 0..16i64 {
                            nodes.push(0x10000 + (li << 6) + (size << 4) + k);
                        }
                    }
                }
                "clusterHashFormatVersion=\"1\" "
            } else {
                "clusterHashFormatVersion=\"2\" "
            };
            let nodesStr = nodes
                .iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>()
                .join(",");
            let xml = format!(
                "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<PathOfBuilding>\n<Build level=\"90\" targetVersion=\"3_0\" className=\"Witch\" ascendClassName=\"None\"/>\n<Tree activeSpec=\"1\">\n<Spec treeVersion=\"{verStr}\" classId=\"3\" ascendClassId=\"0\" {attr}nodes=\"{nodesStr}\">\n<Sockets>\n{}</Sockets>\n</Spec>\n</Tree>\n<Items activeItemSet=\"1\">\n{items}</Items>\n</PathOfBuilding>\n",
                if legacy { &legacySockets } else { &sockets }
            );
            out.push((
                format!(
                    "synth_{verStr}_{}_v{variant}{}",
                    match layout {
                        Layout::Lms => "lms",
                        Layout::Ms => "ms",
                        Layout::S => "s",
                    },
                    if legacy { "_legacy" } else { "" }
                ),
                xml,
            ));
        }
    }
    out
}

// ---- comparison -------------------------------------------------------------------------------

/// Records whose order follows `pairs(tree.nodes)` / `pairs(tree.sockets)` (random per process
/// in PoB for 3.10+ trees, CCR-0024 §5).
fn order_dependent(k: &str) -> bool {
    let last = k.rsplit('/').next().unwrap_or(k);
    matches!(last, "alloco" | "ext" | "jewels" | "hashList")
}

/// `true` for tree versions before 3.10 (no `root` key in `tree.nodes`, CCR-0024 §5).
fn deterministic_tree(ver: &str) -> bool {
    data()
        .gameVersions
        .treeVersions
        .get(&Name::from(ver))
        .is_some_and(|m| m.num < 3.10)
}

/// The `tree.lua` source order of the `nodes` keys (as PT.rs / PS-A.rs read it).
fn source_node_order(src: &std::path::Path, ver: &str) -> Option<Vec<i64>> {
    let text = std::fs::read(src.join("TreeData").join(ver).join("tree.lua")).ok()?;
    let b = &text[..];
    let (mut i, mut depth) = (0usize, 0usize);
    let mut nodes_depth: Option<usize> = None;
    let mut keys = Vec::new();
    while i < b.len() {
        match b[i] {
            b'"' => {
                i += 1;
                while i < b.len() && b[i] != b'"' {
                    if b[i] == b'\\' {
                        i += 1;
                    }
                    i += 1;
                }
            }
            b'{' => depth += 1,
            b'}' => {
                if nodes_depth == Some(depth) {
                    return Some(keys);
                }
                depth -= 1;
            }
            b'[' if depth == 1 && nodes_depth.is_none() && b[i..].starts_with(b"[\"nodes\"]") => {
                nodes_depth = Some(2);
                i += b"[\"nodes\"]".len();
                continue;
            }
            b'[' if nodes_depth.is_some() && depth == 2 => {
                let rest = &b[i + 1..];
                let n = rest.iter().take_while(|c| c.is_ascii_digit()).count();
                if n > 0 && rest[n] == b']' {
                    keys.push(std::str::from_utf8(&rest[..n]).unwrap().parse().unwrap());
                }
            }
            _ => {}
        }
        i += 1;
    }
    None
}

#[derive(Default)]
struct Tally {
    total: BTreeMap<String, usize>,
    bad: BTreeMap<String, usize>,
    info: BTreeMap<String, (usize, usize)>,
    shown: usize,
}

fn kind_of(k: &str) -> String {
    k.split('/')
        .filter(|p| p.parse::<f64>().is_err())
        .collect::<Vec<_>>()
        .join("/")
}

fn sorted_csv(s: Option<&String>) -> Option<Vec<String>> {
    s.map(|s| {
        let mut v: Vec<String> = unesc(s)
            .split(',')
            .filter(|x| !x.is_empty())
            .map(ToOwned::to_owned)
            .collect();
        v.sort();
        v
    })
}

fn compare(
    t: &mut Tally,
    tag: &str,
    lua: &BTreeMap<String, String>,
    rust: &BTreeMap<String, String>,
    det: bool,
) -> usize {
    let keys: BTreeSet<&String> = lua.keys().chain(rust.keys()).collect();
    let mut bad = 0usize;
    for k in keys {
        let kind = kind_of(k);
        let (l, r) = (lua.get(k), rust.get(k));
        let (kind, ok) = if order_dependent(k) && !det {
            let e = t.info.entry(kind.clone()).or_default();
            e.1 += 1;
            if l == r {
                e.0 += 1;
            }
            (format!("{kind}#set"), sorted_csv(l) == sorted_csv(r))
        } else {
            (kind, l == r)
        };
        *t.total.entry(kind.clone()).or_default() += 1;
        if !ok {
            *t.bad.entry(kind).or_default() += 1;
            bad += 1;
            if t.shown < 40 {
                t.shown += 1;
                eprintln!(
                    "{tag} MISMATCH {k}\n  lua  {:?}\n  rust {:?}",
                    l.map(|s| unesc(s)),
                    r.map(|s| unesc(s))
                );
            }
        }
    }
    bad
}

fn run_lua(pob: &mut JitPob, code: &str, what: &str) -> String {
    match pob.run(code) {
        Ok(v) => match v.into_iter().next() {
            Some(Ret::Str(s)) => s,
            other => panic!("{what}: oracle returned {other:?}"),
        },
        Err(e) => panic!("{what}: oracle: {e}"),
    }
}

#[test]
fn passive_spec_cluster_acceptance() {
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
    let only: Option<Vec<String>> = std::env::var("PSC_BUILDS")
        .ok()
        .map(|s| s.split(',').map(|v| v.trim().to_owned()).collect());
    let mut files: Vec<PathBuf> = std::fs::read_dir(parity.join("builds"))
        .unwrap()
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().is_some_and(|e| e == "xml"))
        .filter(|p| {
            only.as_ref().is_none_or(|o| {
                o.iter().any(|s| {
                    p.file_name()
                        .unwrap()
                        .to_string_lossy()
                        .contains(s.as_str())
                })
            })
        })
        .collect();
    files.sort();
    let mut builds: Vec<(String, String)> = files
        .iter()
        .map(|f| {
            (
                f.file_stem().unwrap().to_string_lossy().to_string(),
                std::fs::read_to_string(f).unwrap(),
            )
        })
        .filter(|(_, t)| t.contains("Cluster Jewel"))
        .collect();
    if builds.is_empty() {
        return skip("no cluster builds in .parity/builds");
    }
    let corpus_n = builds.len();
    if std::env::var("PSC_SYNTH").map_or(true, |v| v != "0") {
        builds.extend(synthetic_builds());
    }

    let t0 = std::time::Instant::now();
    // the port side of the oracle's PS-B no-op (LIB; CCR-0052 §12)
    set_spec_stubs(SpecStubs {
        build_all_depends_and_paths: true,
        ..SpecStubs::NONE
    });
    let mut pob = JitPob::boot_oracle(&src, None).unwrap_or_else(|e| panic!("boot: {e}"));
    pob.run(LIB).unwrap_or_else(|e| panic!("{e}"));
    eprintln!("oracle boot: {:.1}s", t0.elapsed().as_secs_f64());
    set_window_title_subtext_hook(Some(Box::new(|s: &str| {
        TITLE.with(|t| *t.borrow_mut() = Some(s.to_owned()));
    })));

    let mut preloaded: BTreeSet<String> = BTreeSet::new();
    let mut versions: BTreeSet<String> = BTreeSet::new();
    let mut t = Tally::default();
    let (mut specs, mut subgraphs, mut clusterNodes, mut legacySpecs) =
        (0usize, 0usize, 0usize, 0usize);
    let (mut legacyMapped, mut legacyRemapped, mut legacyJewels) = (0usize, 0usize, 0usize);
    let mut bad_specs: Vec<String> = Vec::new();
    for (stem, text) in &builds {
        let ser_xml = run_lua(&mut pob, &format!("return __psc.load({text:?})"), stem);
        let nodes = parse_xml(&ser_xml);
        let items = nodes.iter().find(|n| &*n.elem == "Items");
        let build = Build::new(data());
        load_items(&build, items);
        for (si, node) in nodes.iter().filter(|n| &*n.elem == "Spec").enumerate() {
            specs += 1;
            let ver = tree_version_of(node);
            versions.insert(ver.to_string());
            let det = deterministic_tree(&ver);
            if det && preloaded.insert(ver.to_string()) {
                // the tree as PoB builds it from tree.lua (CCR-0024 §5)
                let order = source_node_order(&src, &ver).expect("tree.lua node order");
                setJewelRadiiGlobally(&ver);
                main_tree_insert(ver, new_passive_tree_with_node_order(ver, Some(&order)));
            }
            let _ = main_LoadTree(ver);
            if node
                .attrib
                .get("clusterHashFormatVersion")
                .map_or(node.attrib.contains_key("nodes"), |v| &**v == "1")
            {
                legacySpecs += 1;
            }
            let lua = parse_records(&run_lua(
                &mut pob,
                &format!("return __psc.dumpSafe({})", si + 1),
                &format!("{stem}#{si}"),
            ));
            let rust = pob_lua::error::catch(|| rust_dump(&build, node))
                .unwrap_or_else(|e| BTreeMap::from([("ERR".to_owned(), esc(&format!("{e:?}")))]));
            if lua.contains_key("ERR") || rust.contains_key("ERR") {
                eprintln!(
                    "{stem}#{si}: error: lua {:?} rust {:?}",
                    lua.get("ERR").map(|s| unesc(s)),
                    rust.get("ERR").map(|s| unesc(s))
                );
                let both = lua.contains_key("ERR") && rust.contains_key("ERR");
                *t.total.entry("ERR".to_owned()).or_default() += 1;
                if !both {
                    *t.bad.entry("ERR".to_owned()).or_default() += 1;
                    bad_specs.push(format!("{stem}#{si} (error on one side)"));
                }
                continue;
            }
            if let Some(m) = lua.get("legacy/map") {
                let m = unesc(m);
                legacyMapped += m
                    .split(',')
                    .filter(|e| e.split_once('=').is_some_and(|(a, b)| a != b))
                    .count();
            }
            if let (Some(a), Some(b)) = (lua.get("legacy/presub"), lua.get("legacy/sub")) {
                if a != b {
                    legacyRemapped += 1;
                }
            }
            if let (Some(a), Some(b)) = (lua.get("preload/jewels"), lua.get("legacy/jewels")) {
                if a != b {
                    legacyJewels += 1;
                }
            }
            for (k, v) in &lua {
                if k.starts_with("a3/") && k.ends_with("/nodes") {
                    subgraphs += 1;
                    clusterNodes += unesc(v).split(',').filter(|x| !x.is_empty()).count();
                }
            }
            let n = compare(&mut t, &format!("{stem}#{si}"), &lua, &rust, det);
            if n > 0 {
                bad_specs.push(format!("{stem}#{si} ({n})"));
            }
        }
    }

    // function probes (they mutate tree nodes shared with later specs: run last)
    let tr_l = run_lua(&mut pob, "return __psc.translate()", "translate");
    let spec = {
        let build = Build::new(data());
        new_PassiveSpec(&build, data().gameVersions.defaultTreeVersion, false)
    };
    let mut tr_bad = 0usize;
    let mut tr_n = 0usize;
    for line in tr_l.split('\n') {
        let (lhs, want) = line.split_once('=').unwrap();
        let p: Vec<f64> = lhs.split(':').map(|x| x.parse().unwrap()).collect();
        let got = spec.TranslateClusterOrbitIndex(p[2], p[0], p[1]);
        tr_n += 1;
        if tostr_num_opt(got) != want {
            tr_bad += 1;
            eprintln!("translate {lhs}: lua {want} rust {}", tostr_num_opt(got));
        }
    }
    let keystones = ["Resolute Technique", "Elemental Overload", "Iron Reflexes"];
    let ks_lua = format!(
        "{{ {} }}",
        keystones
            .iter()
            .map(|k| format!("{k:?}"))
            .collect::<Vec<_>>()
            .join(", ")
    );
    let mut probe_bad = 0usize;
    for ver in &versions {
        let lua = parse_records(&run_lua(
            &mut pob,
            &format!("return __psc.probe({ver:?}, {ks_lua})"),
            &format!("probe {ver}"),
        ));
        let build = Build::new(data());
        let rust = rust_probe(&build, Name::from(ver.as_str()), &keystones);
        probe_bad += compare(&mut t, &format!("probe {ver}"), &lua, &rust, true);
    }

    eprintln!(
        "PS-C: {} builds ({corpus_n} corpus, {} synthetic), {specs} specs ({legacySpecs} legacy), {subgraphs} subgraphs / {clusterNodes} cluster nodes after the last round, tree versions {:?}, {:.1}s",
        builds.len(),
        builds.len() - corpus_n,
        versions,
        t0.elapsed().as_secs_f64()
    );
    eprintln!(
        "  legacy: {legacyMapped} legacy -> current mappings registered, {legacyRemapped} specs whose allocSubgraphNodes and {legacyJewels} whose jewels the remap changed"
    );
    let mut total = 0usize;
    let mut bad = 0usize;
    for (k, n) in &t.total {
        let b = t.bad.get(k).copied().unwrap_or(0);
        total += n;
        bad += b;
        eprintln!("  {k:<32} {:>7}/{n}", n - b);
    }
    for (k, (ok, n)) in &t.info {
        eprintln!("  (in order, informational) {k:<20} {ok}/{n}");
    }
    eprintln!(
        "  translate                        {}/{tr_n}",
        tr_n - tr_bad
    );
    eprintln!("PS-C total: {}/{total} records equal; specs with differences: {bad_specs:?}; probes with differences: {probe_bad}", total - bad);
    assert!(subgraphs > 0, "no subgraph was built");
    assert_eq!(tr_bad, 0, "TranslateClusterOrbitIndex differs");
    assert_eq!(bad, 0, "{bad} records differ");
}
