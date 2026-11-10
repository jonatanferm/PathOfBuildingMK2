//! Unit PS-A acceptance (calc doc §11.1, row PS-A): `pob_calc::classes::passive_spec`
//! (Classes/PassiveSpec.lua 1-1091) against PoB's own `PassiveSpec` class under LuaJIT
//! (interpreter, `JitPob::boot_oracle`), on every `<Spec>` of the all57 corpus
//! (`.parity/builds`).
//!
//! `BuildAllDependsAndPaths` (PS-B), `BuildClusterJewelGraphs` and `ReplaceNode` (PS-C) are
//! replaced by no-ops on both sides ("stubbed" mode): the oracle replaces the methods, the port
//! sets `passive_spec::set_spec_stubs` (CCR-0052 §12), so both sides run exactly PS-A's code
//! (PS-B / PS-C have their own tests). For every spec both sides:
//!
//! * `new("PassiveSpec"):PassiveSpec(build, ver)` + `Load(<Spec>)` (the XML node is PoB's own
//!   `common.xml.ParseXML` result, serialised and rebuilt as a `XmlNode`), then record the
//!   class selection, the allocated set / order / flags, mastery selections, jewels, hash
//!   overrides, subgraph nodes, `CountAllocNodes`, the mastery counters and mastery `sd`;
//! * `Save` (attributes and children), `EncodeURL`, `DecodeURL` of that URL and of the XML's
//!   legacy `<URL>`, `DecodePoePlannerURL` of a poeplanner link built from the spec;
//! * with every allocated node marked `connectedToStart`: `SetNodeDistanceToClassStart` and
//!   `GetShortestPathToClassStart` for every allocated node, `IsClassConnected` and
//!   `ConnectToClass` for every class, `GetSocketedJewel`, `NodesInIntuitiveLeapLikeRadius`
//!   (every jewel radius) and `BuildSplitPersonalityPath` with stand-in jewels,
//!   `AddMasteryEffectOptionsToNode` on every unallocated mastery, `DeallocSingleNode` on every
//!   allocated mastery, `AllocNode`/`DeallocNode` on the unallocated neighbours (with a
//!   one-node `path` / `depends`), and the `convert` branch of `Init` (to the latest tree).
//!
//! Orders that follow `pairs(tree.nodes)` (the allocation order after `ResetNodes`' dead
//! keys, the URL bytes, `nodesInRadius`) are random per process in PoB for 3.10+ trees
//! (CCR-0024 §5): for those builds they are compared as sets (asserted) and in order
//! (reported). For older trees the port loads the tree with the `tree.lua` source node order
//! and the orders are asserted too. `mastkey` (the `modKey` of mastery options, from the native
//! ModParser) is asserted like every other record. Comparing against an unstubbed
//! PoB (real PS-B) needs the build's items (PS-B indexes `itemsTab.items` for every jewel) and
//! is left to PS-B / BM-A.
//!
//! Port-only checks on every spec (no oracle side): each spec node has its own `linked` and
//! `depends`, shared neither with another spec's node nor with the tree node (CCR-0052 §1);
//! `ImportFromNodeList` with a tree-version switch (latest tree -> the spec's) and with a
//! `className` gives the same state as a plain import on a fresh spec.
//!
//! Fails without a PoB checkout, `data/dump` or the corpus (`pob_testpaths::require`;
//! `POB_ALLOW_SKIP=1` skips). `PSA_BUILDS=substr,...` limits the builds.
#![allow(non_snake_case, clippy::too_many_lines)]

use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;
use std::rc::Rc;

use pob_calc::classes::passive_spec::{
    base64_encode, main_LoadTree, main_tree_insert, new_PassiveSpec, set_spec_stubs, SpecStubs,
};
use pob_calc::classes::passive_tree::{new_passive_tree_with_node_order, setJewelRadiiGlobally};
use pob_calc::classes::{Item, Node, NodeRef, PassiveSpecRef};
use pob_calc::lua_table::{Key, LStr, Table, Val};
use pob_calc::model::{Build, BuildRef, XmlChild, XmlNode};
use pob_gamedata::{data, Name};
use pob_lua::{tostring_num, Field, Shared};
use pob_oracle::headless::{Engine, Ret};
use pob_oracle::jit::JitPob;
use pob_oracle::oracle::{find_parity_dir, find_pob_src, find_up};

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

/// The stand-in keystone of the impossible-escape check.
const KEYSTONE: &str = "Resolute Technique";

const LIB: &str = r#"
ConPrintf = function() end
if launch then launch.ShowErrMsg = function() end end
if not common.classes.PassiveSpec then LoadModule("Classes/PassiveSpec") end
__ps = { }
local PSC = common.classes.PassiveSpec
local stubNames = { "BuildAllDependsAndPaths", "BuildClusterJewelGraphs", "ReplaceNode" }
local orig = { }
for _, n in ipairs(stubNames) do orig[n] = PSC[n] end
local function stub(on)
    for _, n in ipairs(stubNames) do
        if on then PSC[n] = function() end else PSC[n] = orig[n] end
    end
    if on then
        -- the counters PS-B initialises (the port's fields start at 0)
        PSC.BuildAllDependsAndPaths = function(self)
            self.allocatedMasteryCount = self.allocatedMasteryCount or 0
            self.allocatedMasteryTypeCount = self.allocatedMasteryTypeCount or 0
            self.allocatedMasteryTypes = self.allocatedMasteryTypes or { }
        end
    end
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
function __ps.load(xmlText)
    local dbXML = common.xml.ParseXML(xmlText)
    __ps.specNodes = { }
    local out = { }
    for _, top in ipairs(dbXML) do
        if type(top) == "table" and top.elem == "PathOfBuilding" then
            for _, sec in ipairs(top) do
                if type(sec) == "table" and sec.elem == "Tree" then
                    for _, sp in ipairs(sec) do
                        if type(sp) == "table" and sp.elem == "Spec" then
                            __ps.specNodes[#__ps.specNodes + 1] = sp
                            ser(sp, out)
                        end
                    end
                end
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
local function sdl(sd) return table.concat(sd or { }, "/") end
local function state(spec, pre, emit)
    emit(pre .. "cls", table.concat({ tostring(spec.curClassId), tostring(spec.curAscendClassId), tostring(spec.curSecondaryAscendClassId),
        tostring(spec.curClassName), tostring(spec.curAscendClassName), tostring(spec.curAscendClassBaseName), tostring(spec.curSecondaryAscendClassName) }, "|"))
    emit(pre .. "allocset", joinNums(sortedKeys(spec.allocNodes)))
    emit(pre .. "alloco", keysInOrder(spec.allocNodes))
    local flags = { }
    for id, n in pairs(spec.nodes) do if n.alloc then flags[#flags + 1] = id end end
    table.sort(flags)
    emit(pre .. "allocflag", joinNums(flags))
    emit(pre .. "mastery", kvOrder(spec.masterySelections))
    emit(pre .. "masteryset", kvSorted(spec.masterySelections))
    emit(pre .. "jewels", kvOrder(spec.jewels))
    local ov = { }
    for id, n in pairs(spec.hashOverrides) do
        ov[#ov + 1] = tostring(id) .. ":" .. tostring(n.id) .. ":" .. tostring(n.dn) .. ":" .. tostring(n.isTattoo == true) .. ":" .. tostring(n.overrideType) .. ":" .. sdl(n.sd)
    end
    emit(pre .. "overrides", table.concat(ov, ","))
    emit(pre .. "subgraph", joinNums(spec.allocSubgraphNodes))
    local u, a, s, so = spec:CountAllocNodes()
    emit(pre .. "count", table.concat({ u, a, s, so }, ","))
    emit(pre .. "mcount", tostring(spec.allocatedMasteryCount) .. "," .. tostring(spec.allocatedMasteryTypeCount) .. "," .. kvSorted(spec.allocatedMasteryTypes or { }))
    local ms = { }
    for _, id in ipairs(sortedKeys(spec.allocNodes)) do
        local n = spec.allocNodes[id]
        if n.type == "Mastery" then
            ms[#ms + 1] = id .. ":" .. sdl(n.sd) .. ":" .. tostring(n.allMasteryOptions == true) .. ":" .. sdl(n.reminderText)
        end
    end
    emit(pre .. "msd", table.concat(ms, ","))
end
local build = { itemsTab = { items = { } }, treeTab = { } }
local function newSpec(ver) return new("PassiveSpec"):PassiveSpec(build, ver) end
function __ps.dump(i, ppUrl)
    stub(true)
    build.itemsTab.items = { }
    build.spec = nil
    local out = { }
    local function emit(k, v) out[#out + 1] = k .. "\t" .. esc(v) end
    local node = __ps.specNodes[i]
    local ver = node.attrib.treeVersion or defaultTreeVersion
    local spec = newSpec(ver)
    spec:Load(node, "corpus")
    state(spec, "load/", emit)
    local saved = { elem = "Spec" }
    spec:Save(saved)
    for _, k in ipairs(sortedKeys(saved.attrib)) do emit("save/attr/" .. k, saved.attrib[k]) end
    for ci, c in ipairs(saved) do local o = { } ser(c, o) emit("save/child/" .. ci, table.concat(o, "\n")) end
    local url = spec:EncodeURL("https://www.pathofexile.com/passive-skill-tree/")
    emit("url", url)
    local s2 = newSpec(ver)
    emit("dec/err", tostring((s2:DecodeURL(url))))
    state(s2, "dec/", emit)
    for _, c in ipairs(node) do
        if type(c) == "table" and c.elem == "URL" and type(c[1]) == "string" then
            local s3 = newSpec(ver)
            emit("xurl/err", tostring((s3:DecodeURL(c[1]))))
            state(s3, "xurl/", emit)
            break
        end
    end
    if ppUrl ~= "" then
        local s4 = newSpec(ver)
        emit("pp/ver", tostring((s4:DecodePoePlannerURL(ppUrl, true))))
        emit("pp/err", tostring((s4:DecodePoePlannerURL(ppUrl, false))))
        state(s4, "pp/", emit)
    end
    -- searches over the allocated nodes
    for _, n in pairs(spec.allocNodes) do n.connectedToStart = true end
    local allocIds = sortedKeys(spec.allocNodes)
    local d = { }
    for _, id in ipairs(allocIds) do
        local n = spec.allocNodes[id]
        spec:SetNodeDistanceToClassStart(n)
        d[#d + 1] = id .. "=" .. tostring(n.distanceToClassStart)
    end
    emit("dist", table.concat(d, ","))
    for _, id in ipairs(allocIds) do
        local p = spec:GetShortestPathToClassStart(id)
        emit("spath/" .. id, p and keysInOrder(p) or "nil")
    end
    local classIds = sortedKeys(spec.tree.classes)
    local cl = { }
    for _, c in ipairs(classIds) do cl[#cl + 1] = c .. "=" .. tostring((spec:IsClassConnected(c))) end
    emit("iscc", table.concat(cl, ","))
    local ct = { }
    for _, c in ipairs(classIds) do ct[#ct + 1] = c .. "=" .. tostring((spec:ConnectToClass(c))) end
    emit("ctc", table.concat(ct, ","))
    emit("ctc/allocset", joinNums(sortedKeys(spec.allocNodes)))
    -- jewels (stand-in items)
    local socks = { }
    for _, id in ipairs(sortedKeys(spec.jewels)) do if spec.nodes[id] then socks[#socks + 1] = id end end
    local g = { }
    for _, id in ipairs(socks) do g[#g + 1] = id .. "=" .. tostring(spec:GetSocketedJewel(id) ~= nil) end
    emit("gsj/none", table.concat(g, ","))
    for _, id in ipairs(socks) do
        build.itemsTab.items[spec.jewels[id]] = { jewelRadiusIndex = 1, jewelData = { intuitiveLeapLike = true,
            jewelIncEffectFromClassStart = true, impossibleEscapeKeystone = true, impossibleEscapeKeystones = { [__ps.keystone] = true } } }
    end
    g = { }
    for _, id in ipairs(socks) do g[#g + 1] = id .. "=" .. tostring(spec:GetSocketedJewel(id) ~= nil) end
    emit("gsj", table.concat(g, ","))
    for r = 1, #data.jewelRadius do
        for _, id in ipairs(socks) do build.itemsTab.items[spec.jewels[id]].jewelRadiusIndex = r end
        for _, id in ipairs(socks) do
            local res = spec:NodesInIntuitiveLeapLikeRadius(spec.nodes[id])
            local p, s = { }, { }
            for k, n in ipairs(res) do p[k] = n.id s[k] = n.id end
            table.sort(s)
            emit("leap/" .. id .. "/" .. r, joinNums(p))
            emit("leapset/" .. id .. "/" .. r, joinNums(s))
        end
    end
    spec:BuildSplitPersonalityPath()
    emit("split", joinNums(sortedKeys(spec.splitPersonalityPath)))
    -- mastery options
    local mids = { }
    for id, n in pairs(spec.nodes) do if n.type == "Mastery" and not n.alloc then mids[#mids + 1] = id end end
    table.sort(mids)
    for _, id in ipairs(mids) do
        local n = spec.nodes[id]
        spec:AddMasteryEffectOptionsToNode(n)
        emit("mastopt/" .. id, sdl(n.sd) .. "|" .. tostring(n.allMasteryOptions) .. "|" .. tostring(n.reminderText ~= nil))
        emit("mastkey/" .. id, n.modKey)
    end
    local dm = { }
    for _, id in ipairs(allocIds) do if spec.allocNodes[id] and spec.allocNodes[id].type == "Mastery" then dm[#dm + 1] = id end end
    for _, id in ipairs(dm) do
        local n = spec.nodes[id]
        spec:DeallocSingleNode(n)
        emit("dealloc/" .. id, tostring(n.alloc) .. "|" .. tostring(spec.allocNodes[id] ~= nil) .. "|" .. tostring(spec.masterySelections[id]) .. "|" .. sdl(n.sd) .. "|" .. tostring(n.allMasteryOptions))
    end
    -- AllocNode / DeallocNode on unallocated neighbours (first 20 by id)
    local nb, seen = { }, { }
    for _, id in ipairs(sortedKeys(spec.allocNodes)) do
        for _, o in ipairs(spec.allocNodes[id].linked) do
            if not o.alloc and not seen[o.id] then seen[o.id] = true nb[#nb + 1] = o.id end
        end
    end
    table.sort(nb)
    for k = 21, #nb do nb[k] = nil end
    for k, id in ipairs(nb) do
        local n = spec.nodes[id]
        n.path = { n }
        n.intuitiveLeapLikesAffecting = (k % 3 == 0) and { true } or { }
        spec:AllocNode(n)
        emit("allocnode/" .. id, joinNums(sortedKeys(spec.allocNodes)))
    end
    for _, id in ipairs(nb) do
        local n = spec.nodes[id]
        n.depends = { n }
        spec:DeallocNode(n)
    end
    emit("deallocnode", joinNums(sortedKeys(spec.allocNodes)))
    -- Init's convert branch
    build.spec = spec
    local conv = new("PassiveSpec"):PassiveSpec(build, latestTreeVersion, true)
    emit("conv/ignored", joinNums(sortedKeys(conv.ignoredNodes)))
    build.spec = nil
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

/// The serialised XML of `__ps.load` → nodes.
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

fn ser(node: &XmlNode, out: &mut Vec<String>) {
    out.push(format!("E\t{}", esc(&node.elem)));
    let mut ks: Vec<&LStr> = node.attrib.keys().collect();
    ks.sort();
    for k in ks {
        out.push(format!("A\t{}\t{}", esc(k), esc(&node.attrib[k])));
    }
    for c in &node.children {
        match c {
            XmlChild::Node(n) => ser(n, out),
            XmlChild::Text(t) => out.push(format!("T\t{}", esc(t))),
        }
    }
    out.push("/".to_owned());
}

// ---- Rust side ------------------------------------------------------------------------------

fn opt_name(n: Option<Name>) -> String {
    n.map_or_else(|| "nil".to_owned(), |n| n.to_string())
}

fn tostr_opt(x: f64) -> String {
    if x.is_nan() {
        "nil".to_owned()
    } else {
        tostring_num(x)
    }
}

fn join_nums<I: IntoIterator<Item = f64>>(it: I) -> String {
    it.into_iter()
        .map(tostring_num)
        .collect::<Vec<_>>()
        .join(",")
}

fn sorted_alloc(spec: &PassiveSpecRef) -> Vec<f64> {
    let mut ids: Vec<i64> = spec.allocNodes.with(|m| m.keys());
    ids.sort_unstable();
    ids.into_iter().map(|x| x as f64).collect()
}

fn sdl(node: &Node) -> String {
    node.sd
        .get()
        .snapshot()
        .iter()
        .map(|s| s.to_string())
        .collect::<Vec<_>>()
        .join("/")
}

fn table_strs(v: &Val) -> String {
    match v {
        Val::Table(t) => t
            .ipairs()
            .into_iter()
            .map(|(_, s)| s.str().map(|s| s.to_string()).unwrap_or_default())
            .collect::<Vec<_>>()
            .join("/"),
        _ => String::new(),
    }
}

fn kv_sorted_table(t: &Table) -> String {
    let mut p: Vec<(String, String)> = t
        .pairs()
        .map(|(k, v)| {
            let k = match k {
                Key::S(s) => s.to_string(),
                Key::I(i) => i.to_string(),
                Key::B(b) => b.to_string(),
            };
            let v = match v {
                Val::Num(x) => tostring_num(x),
                other => format!("{other:?}"),
            };
            (k, v)
        })
        .collect();
    p.sort();
    p.into_iter()
        .map(|(k, v)| format!("{k}={v}"))
        .collect::<Vec<_>>()
        .join(",")
}

fn state(spec: &PassiveSpecRef, pre: &str, r: &mut Rec) {
    r.emit(
        format!("{pre}cls"),
        [
            tostr_opt(spec.curClassId.get()),
            tostr_opt(spec.curAscendClassId.get()),
            tostr_opt(spec.curSecondaryAscendClassId.get()),
            opt_name(spec.curClassName.get()),
            opt_name(spec.curAscendClassName.get()),
            opt_name(spec.curAscendClassBaseName.get()),
            opt_name(spec.curSecondaryAscendClassName.get()),
        ]
        .join("|"),
    );
    r.emit(format!("{pre}allocset"), join_nums(sorted_alloc(spec)));
    r.emit(
        format!("{pre}alloco"),
        join_nums(
            spec.allocNodes
                .with(|m| m.keys())
                .into_iter()
                .map(|x| x as f64),
        ),
    );
    let mut flags: Vec<i64> = spec
        .nodes
        .with(|m| m.pairs())
        .into_iter()
        .filter(|(_, n)| n.alloc.get())
        .map(|(k, _)| k)
        .collect();
    flags.sort_unstable();
    r.emit(
        format!("{pre}allocflag"),
        join_nums(flags.into_iter().map(|x| x as f64)),
    );
    let ms = spec.masterySelections.with(|m| m.pairs());
    r.emit(
        format!("{pre}mastery"),
        ms.iter()
            .map(|(k, v)| format!("{k}={}", tostring_num(*v)))
            .collect::<Vec<_>>()
            .join(","),
    );
    let mut mss = ms.clone();
    mss.sort_by_key(|(k, _)| *k);
    r.emit(
        format!("{pre}masteryset"),
        mss.iter()
            .map(|(k, v)| format!("{k}={}", tostring_num(*v)))
            .collect::<Vec<_>>()
            .join(","),
    );
    r.emit(
        format!("{pre}jewels"),
        spec.jewels
            .with(|m| m.pairs())
            .iter()
            .map(|(k, v)| format!("{k}={}", tostring_num(*v)))
            .collect::<Vec<_>>()
            .join(","),
    );
    r.emit(
        format!("{pre}overrides"),
        spec.hashOverrides
            .with(|m| m.pairs())
            .iter()
            .map(|(k, n)| {
                format!(
                    "{k}:{}:{}:{}:{}:{}",
                    tostring_num(n.id),
                    n.dn.get(),
                    n.isTattoo.get(),
                    opt_name(n.overrideType.get()),
                    sdl(n)
                )
            })
            .collect::<Vec<_>>()
            .join(","),
    );
    r.emit(
        format!("{pre}subgraph"),
        join_nums(spec.allocSubgraphNodes.snapshot()),
    );
    let (u, a, s, so, _, _) = spec.CountAllocNodes();
    r.emit(format!("{pre}count"), join_nums([u, a, s, so]));
    r.emit(
        format!("{pre}mcount"),
        format!(
            "{},{},{}",
            tostring_num(spec.allocatedMasteryCount.get()),
            tostring_num(spec.allocatedMasteryTypeCount.get()),
            kv_sorted_table(&spec.allocatedMasteryTypes)
        ),
    );
    let mut msd = Vec::new();
    for id in sorted_alloc(spec) {
        let n = spec.allocNodes.with(|m| m.get(id as i64)).unwrap();
        if &*n.r#type.get() == "Mastery" {
            msd.push(format!(
                "{}:{}:{}:{}",
                tostring_num(id),
                sdl(&n),
                n.allMasteryOptions.get(),
                table_strs(&n.extra.get("reminderText"))
            ));
        }
    }
    r.emit(format!("{pre}msd"), msd.join(","));
}

fn spec_node(spec: &PassiveSpecRef, id: f64) -> NodeRef {
    spec.nodes.with(|m| m.get(id as i64)).unwrap()
}

fn tree_version_of(node: &XmlNode) -> Name {
    node.attrib.get("treeVersion").map_or_else(
        || data().gameVersions.defaultTreeVersion,
        |s| Name::from(&**s),
    )
}

/// A poeplanner link (PassiveSpec.lua 397-425) of the spec's class, allocated nodes and
/// mastery selections, or `""` when poeplanner knows no version of this tree.
fn poeplanner_url(spec: &PassiveSpecRef) -> String {
    let ver = spec.treeVersion.get();
    let Some((&code, _)) = data()
        .gameVersions
        .poePlannerVersions
        .iter()
        .find(|(_, v)| **v == ver)
    else {
        return String::new();
    };
    let u16le = |b: &mut Vec<u8>, x: f64| {
        let x = x as u32;
        b.push((x & 0xff) as u8);
        b.push((x >> 8) as u8);
    };
    let mut b: Vec<u8> = Vec::new();
    u16le(&mut b, 1.0); // serializationVersion
    b.push(0); // buildType
    b.push(0); // isPoE2
    u16le(&mut b, 1.0); // treeSerializationVersion
    u16le(&mut b, code as f64);
    b.push(spec.curClassId.get() as u8);
    b.push(spec.curAscendClassId.get() as u8);
    b.push(0); // bandit
    let (mut normal, mut cluster, mut asc) = (Vec::new(), Vec::new(), Vec::new());
    for id in sorted_alloc(spec) {
        let n = spec.allocNodes.with(|m| m.get(id as i64)).unwrap();
        let ty = n.r#type.get();
        if &*ty == "ClassStart" || &*ty == "AscendClassStart" {
            continue;
        }
        if id >= 65536.0 {
            cluster.push(id - 65536.0);
        } else if n.ascendancyName.get().is_some() {
            asc.push(id);
        } else {
            normal.push(id);
        }
    }
    for list in [&normal, &cluster, &asc] {
        u16le(&mut b, list.len() as f64);
        for &id in list {
            u16le(&mut b, id);
        }
    }
    let mut ms = spec.masterySelections.with(|m| m.pairs());
    ms.sort_by_key(|(k, _)| *k);
    u16le(&mut b, ms.len() as f64);
    for (m, e) in ms {
        u16le(&mut b, m as f64);
        u16le(&mut b, e);
    }
    u16le(&mut b, 0.0); // attribute choices
    let enc = base64_encode(&b).replace('+', "-").replace('/', "_");
    format!("https://poeplanner.com/{enc}")
}

fn new_spec(build: &BuildRef, ver: Name) -> PassiveSpecRef {
    new_PassiveSpec(build, ver, false)
}

/// Two specs on one tree version: each spec node has its own `linked` and `depends`
/// (Lua: `{ linked = { } }` and `wipeTable(nil)` give fresh tables, PassiveSpec.lua:57/1142),
/// shared neither with the other spec's node nor with the tree node. CCR-0052 §1.
fn assert_own_node_lists(a: &PassiveSpecRef, b: &PassiveSpecRef) {
    let mut n = 0usize;
    for (id, na) in a.nodes.with(|m| m.pairs()) {
        let Some(nb) = b.nodes.with(|m| m.get(id)) else {
            continue;
        };
        let t = na.tree.as_ref().expect("spec node has a tree node");
        for (x, y, what) in [
            (&na, &nb, "spec A vs spec B"),
            (&na, t, "spec A vs tree"),
            (&nb, t, "spec B vs tree"),
        ] {
            assert!(
                !x.depends.get().ptr_eq(&y.depends.get()),
                "node {id}: depends shared ({what})"
            );
            assert!(
                !x.linked.get().ptr_eq(&y.linked.get()),
                "node {id}: linked shared ({what})"
            );
        }
        n += 1;
    }
    assert!(n > 0, "no common spec nodes");
}

/// Port-only checks of two `ImportFromNodeList` branches the oracle records do not reach
/// (PassiveSpec.lua:257-268): the tree-version switch (`Init` again on a live spec, from the
/// latest tree to `ver`; CCR-0052 §10 releases the old node set there) and the `className`
/// lookup. Each must leave the same state as a plain import on a fresh `ver` spec.
fn check_import_branches(build: &BuildRef, spec: &PassiveSpecRef, ver: Name) {
    let hashes = sorted_alloc(spec);
    let effects = Table::new();
    for (k, v) in spec.masterySelections.with(|m| m.pairs()) {
        effects.set(k, v);
    }
    let classId = spec.curClassId.get();
    let ascendId = spec.curAscendClassId.get();
    let secId = spec.curSecondaryAscendClassId.get();
    let sec = if secId.is_nan() { 0.0 } else { secId };
    let none = Table::new();
    let dump = |s: &PassiveSpecRef| {
        let mut r = Rec(BTreeMap::new());
        state(s, "", &mut r);
        r.0
    };
    let fresh = new_spec(build, ver);
    fresh.ImportFromNodeList(None, classId, ascendId, sec, &hashes, &none, &effects, None);
    let want = dump(&fresh);
    let latest = data().gameVersions.latestTreeVersion;
    if latest != ver {
        let sw = new_spec(build, latest);
        let old = sw.nodes.with(|m| m.pairs());
        sw.ImportFromNodeList(
            None,
            classId,
            ascendId,
            sec,
            &hashes,
            &none,
            &effects,
            Some(ver),
        );
        assert!(sw.tree.get().treeVersion == ver, "switch: tree version");
        assert_eq!(dump(&sw), want, "tree-version switch {latest} -> {ver}");
        // the replaced node set is unlinked (CCR-0052 §10)
        assert!(old.iter().all(|(_, n)| n.linked.get().is_empty()));
    }
    let className = spec
        .curAscendClassName
        .get()
        .filter(|_| ascendId > 0.0)
        .or_else(|| spec.curClassName.get());
    if let Some(className) = className {
        if &*className != "None" {
            let cn = new_spec(build, ver);
            cn.ImportFromNodeList(
                Some(className),
                f64::NAN,
                f64::NAN,
                sec,
                &hashes,
                &none,
                &effects,
                None,
            );
            assert_eq!(dump(&cn), want, "className {className}");
        }
    }
}

fn rust_dump(xml: &XmlNode, ppUrl: &str) -> BTreeMap<String, String> {
    let mut r = Rec(BTreeMap::new());
    let build = Build::new(data());
    let ver = tree_version_of(xml);
    let spec = new_spec(&build, ver);
    spec.Load(xml, "corpus");
    state(&spec, "load/", &mut r);
    check_import_branches(&build, &spec, ver);
    let mut saved = XmlNode {
        elem: LStr::from("Spec"),
        ..XmlNode::default()
    };
    spec.Save(&mut saved);
    for (k, v) in &saved.attrib {
        r.emit(format!("save/attr/{k}"), &**v);
    }
    for (ci, c) in saved.children.iter().enumerate() {
        let XmlChild::Node(c) = c else {
            panic!("text child")
        };
        let mut o = Vec::new();
        ser(c, &mut o);
        // the values are escaped already: store raw
        r.0.insert(format!("save/child/{}", ci + 1), esc(&o.join("\n")));
    }
    let url = spec.EncodeURL(Some("https://www.pathofexile.com/passive-skill-tree/"));
    r.emit("url", &url);
    let s2 = new_spec(&build, ver);
    r.emit(
        "dec/err",
        s2.DecodeURL(&url).unwrap_or_else(|| "nil".to_owned()),
    );
    state(&s2, "dec/", &mut r);
    assert_own_node_lists(&spec, &s2);
    for c in &xml.children {
        if let XmlChild::Node(c) = c {
            if &*c.elem == "URL" {
                if let Some(XmlChild::Text(t)) = c.children.first() {
                    let s3 = new_spec(&build, ver);
                    r.emit(
                        "xurl/err",
                        s3.DecodeURL(t).unwrap_or_else(|| "nil".to_owned()),
                    );
                    state(&s3, "xurl/", &mut r);
                    break;
                }
            }
        }
    }
    if !ppUrl.is_empty() {
        let s4 = new_spec(&build, ver);
        r.emit(
            "pp/ver",
            s4.DecodePoePlannerURL(ppUrl, true)
                .unwrap_or_else(|| "nil".to_owned()),
        );
        r.emit(
            "pp/err",
            s4.DecodePoePlannerURL(ppUrl, false)
                .unwrap_or_else(|| "nil".to_owned()),
        );
        state(&s4, "pp/", &mut r);
    }
    for (_, n) in spec.allocNodes.with(|m| m.pairs()) {
        n.connectedToStart.set(true);
    }
    let allocIds = sorted_alloc(&spec);
    let mut d = Vec::new();
    for &id in &allocIds {
        let n = spec.allocNodes.with(|m| m.get(id as i64)).unwrap();
        spec.SetNodeDistanceToClassStart(&n);
        d.push(format!(
            "{}={}",
            tostring_num(id),
            n.distanceToClassStart
                .get()
                .map_or_else(|| "nil".to_owned(), tostring_num)
        ));
    }
    r.emit("dist", d.join(","));
    for &id in &allocIds {
        let p = spec.GetShortestPathToClassStart_set(id);
        r.emit(
            format!("spath/{}", tostring_num(id)),
            p.map_or_else(
                || "nil".to_owned(),
                |p| join_nums(p.keys().into_iter().map(|x| x as f64)),
            ),
        );
    }
    let tree = spec.tree.get();
    let mut classIds: Vec<i64> = tree
        .classes
        .pairs()
        .filter_map(|(k, _)| match k {
            Key::I(i) => Some(i),
            _ => None,
        })
        .collect();
    classIds.sort_unstable();
    r.emit(
        "iscc",
        classIds
            .iter()
            .map(|&c| format!("{c}={}", spec.IsClassConnected(c as f64)))
            .collect::<Vec<_>>()
            .join(","),
    );
    r.emit(
        "ctc",
        classIds
            .iter()
            .map(|&c| format!("{c}={}", spec.ConnectToClass_ret(c as f64)))
            .collect::<Vec<_>>()
            .join(","),
    );
    r.emit("ctc/allocset", join_nums(sorted_alloc(&spec)));
    // jewels (stand-in items)
    let mut jewelIds: Vec<i64> = spec.jewels.with(|m| m.keys());
    jewelIds.sort_unstable();
    let socks: Vec<i64> = jewelIds
        .into_iter()
        .filter(|id| spec.nodes.with(|m| m.contains_key(*id)))
        .collect();
    let gsj = |spec: &PassiveSpecRef| {
        socks
            .iter()
            .map(|&id| format!("{id}={}", spec.GetSocketedJewel(id as f64).0.is_some()))
            .collect::<Vec<_>>()
            .join(",")
    };
    r.emit("gsj/none", gsj(&spec));
    let mut fakes: BTreeMap<u32, Rc<Item>> = BTreeMap::new();
    for &id in &socks {
        let itemId = spec.jewels.with(|m| m.get(id)).unwrap() as u32;
        let ie = Table::new();
        ie.set(KEYSTONE, true);
        let jd = Table::new();
        jd.set("intuitiveLeapLike", true);
        jd.set("jewelIncEffectFromClassStart", true);
        jd.set("impossibleEscapeKeystone", true);
        jd.set("impossibleEscapeKeystones", ie);
        let item = Rc::new(Item {
            jewelRadiusIndex: std::cell::Cell::new(Some(1.0)),
            jewelData: Field::new(Some(jd)),
            ..Item::default()
        });
        build
            .itemsTab
            .items
            .with_mut(|m| m.insert(itemId, item.clone()));
        fakes.insert(itemId, item);
    }
    r.emit("gsj", gsj(&spec));
    let nradius = pob_gamedata::jewelRadius().jewelRadius.len();
    for rad in 1..=nradius {
        for item in fakes.values() {
            item.jewelRadiusIndex.set(Some(rad as f64));
        }
        for &id in &socks {
            let res = spec.NodesInIntuitiveLeapLikeRadius(&spec_node(&spec, id as f64));
            let p: Vec<f64> = res.iter().map(|n| n.id).collect();
            let mut s = p.clone();
            s.sort_by(f64::total_cmp);
            r.emit(format!("leap/{id}/{rad}"), join_nums(p));
            r.emit(format!("leapset/{id}/{rad}"), join_nums(s));
        }
    }
    spec.BuildSplitPersonalityPath();
    let mut split: Vec<i64> = spec
        .splitPersonalityPath
        .get()
        .unwrap()
        .pairs()
        .filter_map(|(k, _)| match k {
            Key::I(i) => Some(i),
            _ => None,
        })
        .collect();
    split.sort_unstable();
    r.emit("split", join_nums(split.into_iter().map(|x| x as f64)));
    // mastery options
    let mut mids: Vec<i64> = spec
        .nodes
        .with(|m| m.pairs())
        .into_iter()
        .filter(|(_, n)| &*n.r#type.get() == "Mastery" && !n.alloc.get())
        .map(|(k, _)| k)
        .collect();
    mids.sort_unstable();
    for id in mids {
        let n = spec_node(&spec, id as f64);
        spec.AddMasteryEffectOptionsToNode(&n);
        let rt = n.extra.get("reminderText");
        r.emit(
            format!("mastopt/{id}"),
            format!("{}|{}|{}", sdl(&n), n.allMasteryOptions.get(), !rt.is_nil()),
        );
        r.emit(format!("mastkey/{id}"), &*n.modKey.get());
    }
    for &id in &allocIds {
        let Some(n) = spec.allocNodes.with(|m| m.get(id as i64)) else {
            continue;
        };
        if &*n.r#type.get() != "Mastery" {
            continue;
        }
        spec.DeallocSingleNode(&n);
        r.emit(
            format!("dealloc/{}", tostring_num(id)),
            format!(
                "{}|{}|{}|{}|{}",
                n.alloc.get(),
                spec.allocNodes.with(|m| m.contains_key(id as i64)),
                spec.masterySelections
                    .with(|m| m.get(id as i64))
                    .map_or_else(|| "nil".to_owned(), tostring_num),
                sdl(&n),
                n.allMasteryOptions.get()
            ),
        );
    }
    // AllocNode / DeallocNode on unallocated neighbours (first 20 by id)
    let mut nb: BTreeSet<i64> = BTreeSet::new();
    for id in sorted_alloc(&spec) {
        let n = spec.allocNodes.with(|m| m.get(id as i64)).unwrap();
        for o in n.linked.get().snapshot() {
            if !o.alloc.get() {
                nb.insert(o.id as i64);
            }
        }
    }
    let nb: Vec<i64> = nb.into_iter().take(20).collect();
    for (k, &id) in nb.iter().enumerate() {
        let n = spec_node(&spec, id as f64);
        n.path.set(Some(Shared::new(vec![n.clone()])));
        let ill = Table::new();
        if (k + 1) % 3 == 0 {
            ill.push(true);
        }
        n.extra.set("intuitiveLeapLikesAffecting", ill);
        spec.AllocNode(&n, None);
        r.emit(format!("allocnode/{id}"), join_nums(sorted_alloc(&spec)));
    }
    for &id in &nb {
        let n = spec_node(&spec, id as f64);
        n.depends.set(Shared::new(vec![n.clone()]));
        spec.DeallocNode(&n);
    }
    r.emit("deallocnode", join_nums(sorted_alloc(&spec)));
    // Init's convert branch
    build.spec.set(spec.clone());
    let conv = new_PassiveSpec(&build, data().gameVersions.latestTreeVersion, true);
    let mut ign: Vec<i64> = conv
        .ignoredNodes
        .pairs()
        .filter_map(|(k, _)| match k {
            Key::I(i) => Some(i),
            _ => None,
        })
        .collect();
    ign.sort_unstable();
    r.emit("conv/ignored", join_nums(ign.into_iter().map(|x| x as f64)));
    drop(conv);
    r.0
}

/// Records whose order follows `pairs(tree.nodes)` (random per process in PoB for 3.10+
/// trees, CCR-0024 §5), with the set-valued twin each is also checked through.
fn order_dependent(k: &str) -> bool {
    let last = k.rsplit('/').next().unwrap_or(k);
    matches!(last, "alloco" | "url" | "nodes")
        || k == "save/child/1"
        || k == "dec/mastery"
        || k.starts_with("leap/")
}

/// `true` for tree versions before 3.10 (no `root` key in `tree.nodes`, CCR-0024 §5).
fn deterministic_tree(ver: &str) -> bool {
    data()
        .gameVersions
        .treeVersions
        .get(&Name::from(ver))
        .is_some_and(|m| m.num < 3.10)
}

/// The `tree.lua` source order of the `nodes` keys (as PT.rs reads it).
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

/// A URL's decoded content as sets (header bytes, then each section sorted).
fn url_norm(url: &str) -> String {
    let b = pob_calc::classes::passive_spec::base64_decode(
        &url.rsplit('/')
            .next()
            .unwrap()
            .replace('-', "+")
            .replace('_', "/"),
    );
    if b.len() < 7 {
        return format!("short:{b:?}");
    }
    let head = &b[..6];
    let mut i = 6;
    let mut take = |w: usize, b: &[u8]| -> Vec<Vec<u8>> {
        let n = b.get(i).copied().unwrap_or(0) as usize;
        i += 1;
        let mut v: Vec<Vec<u8>> = (0..n)
            .map(|k| b.get(i + k * w..i + (k + 1) * w).unwrap_or(&[]).to_vec())
            .collect();
        i += n * w;
        v.sort();
        v
    };
    let nodes = take(2, &b);
    let clusters = take(2, &b);
    let masteries = take(4, &b);
    format!("{head:?}|{nodes:?}|{clusters:?}|{masteries:?}")
}

#[test]
fn passive_spec_acceptance() {
    let Some(src) = pob_testpaths::require(find_pob_src(), "no PoB checkout") else {
        return; // POB_ALLOW_SKIP=1
    };
    if pob_testpaths::require(init_data().then_some(()), "data/dump not found").is_none() {
        return; // POB_ALLOW_SKIP=1
    }
    let Some(parity) = pob_testpaths::require(find_parity_dir(), "no .parity corpus") else {
        return; // POB_ALLOW_SKIP=1
    };
    let src = std::fs::canonicalize(&src).unwrap();
    let only: Option<Vec<String>> = std::env::var("PSA_BUILDS")
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
    if files.is_empty() {
        let _ = pob_testpaths::require(None::<()>, "no builds in .parity/builds");
        return; // POB_ALLOW_SKIP=1
    }

    // the port side of the oracle's `stub(true)` (LIB): PS-B / PS-C bodies are no-ops
    set_spec_stubs(SpecStubs {
        build_all_depends_and_paths: true,
        build_cluster_jewel_graphs: true,
        replace_node: true,
    });
    let t0 = std::time::Instant::now();
    let mut pob = JitPob::boot_oracle(&src, None).unwrap_or_else(|e| panic!("boot: {e}"));
    pob.run(LIB).unwrap_or_else(|e| panic!("{e}"));
    pob.run(&format!("__ps.keystone = {KEYSTONE:?}"))
        .unwrap_or_else(|e| panic!("{e}"));
    eprintln!("oracle boot: {:.1}s", t0.elapsed().as_secs_f64());

    let mut preloaded: BTreeSet<String> = BTreeSet::new();
    let mut total_by_kind: BTreeMap<String, usize> = BTreeMap::new();
    let mut bad_by_kind: BTreeMap<String, usize> = BTreeMap::new();
    let mut info_by_kind: BTreeMap<String, (usize, usize)> = BTreeMap::new();
    let mut specs = 0usize;
    let mut shown = 0usize;
    for file in &files {
        let stem = file.file_stem().unwrap().to_string_lossy().to_string();
        let text = std::fs::read_to_string(file).unwrap();
        let ser_xml = match pob.run(&format!("return __ps.load({text:?})")) {
            Ok(v) => match v.into_iter().next() {
                Some(Ret::Str(s)) => s,
                other => panic!("{stem}: oracle returned {other:?}"),
            },
            Err(e) => panic!("{stem}: oracle: {e}"),
        };
        let nodes = parse_xml(&ser_xml);
        for (si, node) in nodes.iter().enumerate() {
            specs += 1;
            let ver = tree_version_of(node);
            let det = deterministic_tree(&ver);
            if det && preloaded.insert(ver.to_string()) {
                // the tree as PoB builds it from tree.lua (CCR-0024 §5)
                let order = source_node_order(&src, &ver).expect("tree.lua node order");
                setJewelRadiiGlobally(&ver);
                main_tree_insert(ver, new_passive_tree_with_node_order(ver, Some(&order)));
            }
            let _ = main_LoadTree(ver);
            // the poeplanner link from the port's loaded spec (both sides decode the same text)
            let ppUrl = {
                let build = Build::new(data());
                let spec = new_spec(&build, ver);
                spec.Load(node, "corpus");
                poeplanner_url(&spec)
            };
            let lua = match pob.run(&format!("return __ps.dump({}, {ppUrl:?})", si + 1)) {
                Ok(v) => match v.into_iter().next() {
                    Some(Ret::Str(s)) => parse_records(&s),
                    other => panic!("{stem}#{si}: oracle returned {other:?}"),
                },
                Err(e) => panic!("{stem}#{si}: oracle: {e}"),
            };
            let rust = rust_dump(node, &ppUrl);
            let keys: BTreeSet<&String> = lua.keys().chain(rust.keys()).collect();
            let mut spec_bad = 0usize;
            for k in keys {
                let kind = k
                    .split('/')
                    .filter(|p| p.parse::<f64>().is_err())
                    .collect::<Vec<_>>()
                    .join("/");
                let (l, r) = (lua.get(k), rust.get(k));
                if order_dependent(k) && !det {
                    let e = info_by_kind.entry(kind.clone()).or_default();
                    e.1 += 1;
                    if l == r {
                        e.0 += 1;
                    }
                    if kind == "url" || kind == "save/child" {
                        // the URL as sets
                        let norm = |s: Option<&String>| {
                            s.map(|s| {
                                let s = unesc(s);
                                let url = s
                                    .split('\n')
                                    .find(|l| l.starts_with("T\t"))
                                    .map_or(s.as_str(), |l| &l[2..])
                                    .to_owned();
                                url_norm(&url)
                            })
                        };
                        let kk = format!("{kind}#set");
                        *total_by_kind.entry(kk.clone()).or_default() += 1;
                        if norm(l) != norm(r) {
                            *bad_by_kind.entry(kk).or_default() += 1;
                            spec_bad += 1;
                            if shown < 30 {
                                shown += 1;
                                eprintln!(
                                    "{stem}#{si} MISMATCH {k} (as sets)\n  lua  {:?}\n  rust {:?}",
                                    norm(l),
                                    norm(r)
                                );
                            }
                        }
                    } else if k.ends_with("save/attr/nodes") {
                        let set = |s: Option<&String>| {
                            s.map(|s| {
                                let mut v: Vec<i64> =
                                    s.split(',').filter_map(|x| x.parse().ok()).collect();
                                v.sort_unstable();
                                v
                            })
                        };
                        let kk = format!("{kind}#set");
                        *total_by_kind.entry(kk.clone()).or_default() += 1;
                        if set(l) != set(r) {
                            *bad_by_kind.entry(kk).or_default() += 1;
                            spec_bad += 1;
                        }
                    }
                    continue;
                }
                *total_by_kind.entry(kind.clone()).or_default() += 1;
                if l != r {
                    *bad_by_kind.entry(kind.clone()).or_default() += 1;
                    spec_bad += 1;
                    if shown < 30 {
                        shown += 1;
                        let cut = |s: Option<&String>| {
                            s.map_or_else(
                                || "<missing>".to_owned(),
                                |s| s.chars().take(800).collect(),
                            )
                        };
                        eprintln!(
                            "{stem}#{si} MISMATCH {k}\n  lua  {}\n  rust {}",
                            cut(l),
                            cut(r)
                        );
                    }
                }
            }
            eprintln!(
                "{stem}#{si} ({ver}{}): {} records, {spec_bad} mismatches",
                if det { ", ordered" } else { "" },
                lua.len()
            );
        }
    }
    set_spec_stubs(SpecStubs::NONE);
    eprintln!("\n== PS-A: {specs} specs in {} builds ==", files.len());
    for (k, n) in &total_by_kind {
        let b = bad_by_kind.get(k).copied().unwrap_or(0);
        eprintln!("  {k:<22} {:>6}/{n:<6} identical", n - b);
    }
    eprintln!("  reported only (orders random in PoB):");
    for (k, (same, n)) in &info_by_kind {
        eprintln!("  {k:<22} {same:>6}/{n:<6} identical");
    }
    let bad: usize = bad_by_kind.values().sum();
    let total: usize = total_by_kind.values().sum();
    eprintln!("  asserted records: {}/{total} identical", total - bad);
    assert_eq!(bad, 0, "{bad} mismatching records (see above)");
    // coverage and the report-only bucket (HYG2/V2): every build's specs were compared, and the
    // records excused as order-dependent may not differ more than measured
    if only.is_none() {
        assert_eq!(
            (files.len(), specs),
            PSA_COVERAGE,
            "(builds, specs) compared"
        );
        assert!(
            total >= PSA_MIN_RECORDS,
            "{total} asserted records, expected at least {PSA_MIN_RECORDS}"
        );
        for (k, (same, n)) in &info_by_kind {
            let max = PSA_MAX_REPORT_ONLY_MISSES
                .iter()
                .find(|(kk, _)| kk == k)
                .map_or(0, |(_, m)| *m);
            assert!(
                n - same <= max,
                "report-only {k}: {} of {n} differ, bound {max} (PSA_MAX_REPORT_ONLY_MISSES)",
                n - same
            );
        }
    }
}

/// `(builds, specs)` of the corpus; measured by HYG2 on rust-port 746b103.
const PSA_COVERAGE: (usize, usize) = (57, 59);
/// Asserted records over the corpus; measured by HYG2 on rust-port 746b103 (31,969), plus the
/// 16,963 `mastkey` records MPFIX moved from report-only to asserted (rust-port 387db54).
const PSA_MIN_RECORDS: usize = 48_932;
/// Per report-only kind, the records that may differ (unlisted kinds: 0), measured by HYG2 on
/// rust-port 746b103. `leap` is the order-dependent leap list of a tree whose node order is
/// random in PoB (CCR-0024 §5): any order is allowed because the same list is asserted as a set
/// (`leapset`). Everything else matched on every record.
const PSA_MAX_REPORT_ONLY_MISSES: &[(&str, usize)] = &[("leap", usize::MAX)];
