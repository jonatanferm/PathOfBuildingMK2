//! Unit PT acceptance (calc doc §11.1, row PT, level L2): `pob_calc::classes::passive_tree`
//! against PoB's own `PassiveTree` class under LuaJIT (interpreter, `JitPob::boot_oracle`),
//! for every tree version of `treeVersionList` (41).
//!
//! For each version both sides run `data.setJewelRadiiGlobally(ver)` and load the tree
//! (`main:LoadTree(ver)` / `new_PassiveTree(ver)`), then serialise the same records:
//! every node's fields (`id`, `type`, `dn`, `ascendancyName`, `classStartIndex`,
//! `passivePointsGranted`, `o`, `oidx`, `unknown`, `extra`, `modKey`, `sd`, `keystoneMod`),
//! `linkedId`, `modList` (formatMod + source), `mods`, the mastery effects of mastery nodes,
//! positions (`x`, `y`, `angle`; |Δ| ≤ 1e-9), `nodesInRadius` of every socket and tree
//! keystone (as sets; the `pairs()` order separately), `charmSocket`, the keystone /
//! notable / ascendancy / cluster maps, `classNotables`, classes and ascendancy maps,
//! groups, orbit tables, `size`, the jewel radii, `masteryEffects`, legion nodes and
//! additions, tattoo nodes and `idMap`.
//!
//! Mismatches are counted per record kind. The oracle records every `modLib.parseMod` call
//! that `ProcessStats` makes for a node, with its result. A node record (`node`, `ml`, `mods`,
//! `mastery`, `legion`, `tattoo`) that differs although its structural fields agree, and for
//! which at least one of those calls gives a different result in `pob_modparser::parseMod`
//! (the native ModParser: units MP1-MP7), is classified as "parser-dependent" (a separate count
//! in the report); both kinds fail the test (MP0-MP7 are ported: the bound on parser-dependent
//! records is 0). Orders that PoB itself does not fix (`tree.nodes` order: CCR-0024 §5) are
//! reported, not asserted.
//!
//! Fails without a PoB checkout or `data/dump` (`pob_testpaths::require`; `POB_ALLOW_SKIP=1`
//! skips). `PT_VERSIONS=3_29,2_6` limits the versions.
#![allow(non_snake_case, clippy::too_many_lines, clippy::many_single_char_names)]

use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;
use std::rc::Rc;

use pob_calc::classes::passive_tree::{
    new_PassiveTree, new_passive_tree_with_node_order, node_id_val, nodesInRadiusAt,
    setJewelRadiiGlobally,
};
use pob_calc::classes::{ModList, Node, NodeRef, PassiveTree};
use pob_calc::lua_table::{Key, Val};
use pob_gamedata::mod_tools::{formatMod, FUNCTION_TEXT_PREFIX};
use pob_gamedata::{data, n, ModRef, Name};
use pob_lua::{lfmt, tostring_num};
use pob_oracle::headless::{Engine, Ret};
use pob_oracle::jit::JitPob;
use pob_oracle::oracle::{find_pob_src, find_up};
use pob_oracle::runner::OracleLock;

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

/// The Lua serialiser (one `key\tvalue` record per line).
const LIB: &str = r#"
ConPrintf = function() end
__pt = {}
-- Record every modLib.parseMod call per node (ProcessStats), with its result, so the Rust side
-- can tell which nodes saw a parse result that the native ModParser does not reproduce yet.
do
    local PTC = common.classes.PassiveTree
    local origPS, origParse = PTC.ProcessStats, modLib.parseMod
    local cur
    __pt.parsed = setmetatable({ }, { __mode = "k" })
    local function pres(list, extra)
        local p = { }
        if list then for _, m in ipairs(list) do p[#p + 1] = modLib.formatMod(m) end end
        return (list and "L" or "N") .. "/" .. tostring(extra) .. "/" .. table.concat(p, ";")
    end
    PTC.ProcessStats = function(self, node, startIndex)
        local prev = cur
        cur = node
        __pt.parsed[node] = { }
        origPS(self, node, startIndex)
        cur = prev
    end
    modLib.parseMod = function(line, isComb)
        local list, extra = origParse(line, isComb)
        if cur then
            local t = __pt.parsed[cur]
            t[#t + 1] = line .. "\3" .. pres(list, extra)
        end
        return list, extra
    end
end
function __pt.dump(ver)
    -- a fresh build, also of the latest tree (Main.lua:152 built it before the parse recorder)
    main.tree[ver] = nil
    data.setJewelRadiiGlobally(ver)
    local tree = main:LoadTree(ver)
    local out = {}
    -- formatValue prints a function value (jewel funcs) with its address; the Rust side prints
    -- pob_gamedata::mod_tools::FUNCTION_TEXT for it, and `fn_text` maps both to "table"
    local function emit(k, v) out[#out + 1] = k .. "\t" .. (v:gsub("\n", "\\n"):gsub("function: 0x%x+", "table")) end
    local function esc(s) if s == nil then return "nil" end return (tostring(s):gsub("\n", "\\n")) end
    local function f17(x) if x == nil then return "nil" end return string.format("%.17g", x) end
    local function ms(m) return modLib.formatMod(m) .. "@" .. tostring(m.source) end
    local function ml(list)
        if not list then return "nil" end
        local p = {}
        for _, m in ipairs(list) do p[#p + 1] = ms(m) end
        return table.concat(p, ";")
    end
    local function plist(node) return table.concat(__pt.parsed[node] or { }, "\4") end
    local function sdl(sd)
        if not sd then return "nil" end
        local p = {}
        for i, l in ipairs(sd) do p[i] = esc(l) end
        return table.concat(p, "\1")
    end
    local function modsl(mods)
        if not mods then return "nil" end
        local p = {}
        for i, e in ipairs(mods) do
            p[i] = (e.combined and "C" or "") .. "/" .. tostring(e.extra) .. "/" .. ml(e.list)
        end
        return table.concat(p, "|")
    end
    local function nodeRec(node)
        return table.concat({ tostring(node.id), tostring(node.type), esc(node.dn or ""), tostring(node.ascendancyName),
            tostring(node.classStartIndex), tostring(node.passivePointsGranted), f17(node.o), f17(node.oidx),
            tostring(node.unknown), tostring(node.extra), esc(node.modKey), sdl(node.sd),
            node.keystoneMod and ms(node.keystoneMod) or "nil" }, "\2")
    end
    local function sortedKeys(t)
        local ks = {}
        for k in pairs(t) do ks[#ks + 1] = k end
        table.sort(ks, function(a, b) if type(a) == type(b) then return a < b end return type(a) < type(b) end)
        return ks
    end
    local function ids(t)
        local p = {}
        for _, k in ipairs(sortedKeys(t)) do p[#p + 1] = tostring(k) end
        return table.concat(p, ",")
    end
    local function idsOrder(t)
        local p = {}
        for k in pairs(t) do p[#p + 1] = tostring(k) end
        return table.concat(p, ",")
    end
    local nirSeen = {}
    local function nir(node)
        if nirSeen[node] then return end
        nirSeen[node] = true
        if node.nodesInRadius then
            for r, t in pairs(node.nodesInRadius) do
                emit("nir:" .. tostring(node.id) .. ":" .. r, ids(t))
                emit("niro:" .. tostring(node.id) .. ":" .. r, idsOrder(t))
            end
        end
        if node.attributesInRadius then
            local c = 0
            for r, t in pairs(node.attributesInRadius) do c = c + 1 end
            emit("air:" .. tostring(node.id), tostring(c))
        end
    end
    local order = {}
    for id in pairs(tree.nodes) do order[#order + 1] = tostring(id) end
    emit("order", table.concat(order, ","))
    for _, id in ipairs(sortedKeys(tree.nodes)) do
        local node = tree.nodes[id]
        local k = tostring(id)
        emit("node:" .. k, nodeRec(node))
        local l = {}
        for i, x in ipairs(node.linkedId) do l[i] = tostring(x) end
        emit("link:" .. k, table.concat(l, ","))
        emit("ml:" .. k, ml(node.modList))
        emit("mods:" .. k, modsl(node.mods))
        emit("parse:" .. k, plist(node))
        emit("pos:" .. k, f17(node.x) .. "," .. f17(node.y) .. "," .. f17(node.angle))
        if node.masteryEffects then
            local p = {}
            for i, e in ipairs(node.masteryEffects) do p[i] = tostring(e.effect) .. ":" .. sdl(e.stats) end
            emit("meff:" .. k, table.concat(p, "|"))
        end
        if node.type == "Socket" then
            emit("charm:" .. k, tostring(node.charmSocket))
            nir(node)
        end
        emit("grp:" .. k, node.group and (tostring(node.group.ascendancyName) .. "|" .. tostring(node.group.isAscendancyStart)) or "nil")
    end
    for name, node in pairs(tree.keystoneMap) do
        emit("km:" .. name, tostring(node.id))
        if node.nodesInRadius then nir(node) end
    end
    for name, node in pairs(tree.notableMap) do emit("nm:" .. name, tostring(node.id)) end
    for name, node in pairs(tree.ascendancyMap) do emit("am:" .. name, tostring(node.id)) end
    for name, node in pairs(tree.clusterNodeMap) do emit("cm:" .. name, tostring(node.id)) end
    emit("sockets", ids(tree.sockets))
    for name, list in pairs(tree.classNotables) do
        local p = {}
        for i, dn in ipairs(list) do p[i] = esc(dn) end
        emit("cn:" .. name, table.concat(p, "|"))
    end
    for classId, class in pairs(tree.classes) do
        emit("class:" .. tostring(classId), tostring(class.name) .. "|" .. tostring(class.startNodeId))
        for ascId, asc in pairs(class.classes) do
            emit("asc:" .. tostring(classId) .. ":" .. tostring(ascId), tostring(asc.name) .. "|" .. tostring(asc.startNodeId) .. "|" .. tostring(asc.id))
        end
    end
    for name, e in pairs(tree.classNameMap) do emit("cnm:" .. name, tostring(e)) end
    for name, e in pairs(tree.ascendNameMap) do
        emit("anm:" .. name, tostring(e.classId) .. "|" .. tostring(e.ascendClassId) .. "|" .. tostring(e.ascendClass.name) .. "|" .. tostring(e.class.name))
    end
    for name, e in pairs(tree.internalAscendNameMap) do
        emit("ianm:" .. name, tostring(e.classId) .. "|" .. tostring(e.ascendClassId) .. "|" .. tostring(e.ascendClass.name))
    end
    if tree.secondaryAscendNameMap then
        for name, e in pairs(tree.secondaryAscendNameMap) do
            emit("sanm:" .. name, tostring(e.classId) .. "|" .. tostring(e.ascendClassId) .. "|" .. tostring(e.ascendClass.name))
        end
    end
    if tree.alternate_ascendancies then
        for k, a in pairs(tree.alternate_ascendancies) do emit("alt:" .. tostring(k), tostring(a.id)) end
    end
    for _, gid in ipairs(sortedKeys(tree.groups)) do
        local g = tree.groups[gid]
        local oo = {}
        for _, o in ipairs(sortedKeys(g.oo or {})) do oo[#oo + 1] = tostring(o) end
        emit("group:" .. tostring(gid), tostring(g.ascendancyName) .. "|" .. tostring(g.isAscendancyStart) .. "|" .. table.concat(oo, ",") .. "|" .. tostring(g.n and #g.n))
    end
    local p = {}
    for i, x in ipairs(tree.skillsPerOrbit) do p[#p + 1] = f17(x) end
    emit("skillsPerOrbit", table.concat(p, ","))
    p = {}
    for i, x in ipairs(tree.orbitRadii) do p[#p + 1] = f17(x) end
    emit("orbitRadii", table.concat(p, ","))
    for o, angles in ipairs(tree.orbitAnglesByOrbit) do
        local q = {}
        for i, a in ipairs(angles) do q[i] = f17(a) end
        emit("angles:" .. o, table.concat(q, ","))
    end
    emit("size", f17(tree.size))
    p = { f17(data.maxJewelRadius) }
    for i, r in ipairs(data.jewelRadius) do p[#p + 1] = f17(r.inner) .. "/" .. f17(r.outer) .. "/" .. f17(r.innerSquared) .. "/" .. f17(r.outerSquared) end
    emit("jewelRadius", table.concat(p, ","))
    for _, id in ipairs(sortedKeys(tree.masteryEffects)) do
        local m = tree.masteryEffects[id]
        emit("mastery:" .. tostring(id), nodeRec(m) .. "\2" .. ml(m.modList) .. "\2" .. modsl(m.mods))
        emit("parse:mastery:" .. tostring(id), plist(m))
    end
    for i, node in ipairs(tree.legion.nodes) do
        emit("legion:" .. i, nodeRec(node) .. "\2" .. ml(node.modList) .. "\2" .. modsl(node.mods))
        emit("parse:legion:" .. i, plist(node))
    end
    for i, a in ipairs(tree.legion.additions) do emit("ladd:" .. i, tostring(a.id) .. "|" .. esc(a.dn)) end
    for name, node in pairs(tree.tattoo.nodes) do
        emit("tattoo:" .. name, nodeRec(node) .. "\2" .. ml(node.modList) .. "\2" .. modsl(node.mods))
        emit("parse:tattoo:" .. name, plist(node))
    end
    for id, dn in pairs(tree.tattoo.idMap) do emit("tidmap:" .. tostring(id), esc(dn)) end
    main.tree[ver] = nil
    tree = nil
    collectgarbage()
    return table.concat(out, "\n")
end
"#;

// ---------------------------------------------------------------------------------------
// Rust serialiser (the same records)
// ---------------------------------------------------------------------------------------

fn f17(x: f64) -> String {
    if x.is_nan() {
        "nil".to_owned()
    } else {
        lfmt!("%.17g", x)
    }
}

fn tostr(v: &Val) -> String {
    match v {
        Val::Nil => "nil".to_owned(),
        Val::Bool(b) => b.to_string(),
        Val::Num(x) => tostring_num(*x),
        Val::Str(s) => s.to_string(),
        Val::Obj(_) => "userdata".to_owned(),
        _ => "table".to_owned(),
    }
}

fn esc(s: &str) -> String {
    s.replace('\n', "\\n")
}

fn key_text(k: Key) -> String {
    match k {
        Key::S(n) => n.to_string(),
        Key::I(i) => i.to_string(),
        Key::B(b) => b.to_string(),
        Key::F(f) => tostring_num(f.get()),
    }
}

/// The Lua side's `gsub("function: 0x%x+", "table")` (a jewel closure's address) on the Rust
/// side's text for a closure (`mod_tools::function_text`: `function: 0x` and a per-closure hex
/// identity, FUP2).
fn fn_text(s: String) -> String {
    let mut out = String::with_capacity(s.len());
    let mut rest = s.as_str();
    while let Some(i) = rest.find(FUNCTION_TEXT_PREFIX) {
        let after = &rest[i + FUNCTION_TEXT_PREFIX.len()..];
        let hex = after
            .find(|c: char| !c.is_ascii_hexdigit())
            .unwrap_or(after.len());
        out.push_str(&rest[..i]);
        // `%x+` needs at least one digit
        out.push_str(if hex == 0 {
            FUNCTION_TEXT_PREFIX
        } else {
            "table"
        });
        rest = &after[hex..];
    }
    out.push_str(rest);
    out
}

fn ms(m: &ModRef) -> String {
    format!("{}@{}", formatMod(m), m.source.as_deref().unwrap_or("nil"))
}

fn ml_store(ml: &ModList) -> String {
    ml.ipairs().iter().map(ms).collect::<Vec<_>>().join(";")
}

fn ml_table(list: &Val) -> String {
    match list {
        Val::Table(t) => t
            .ipairs()
            .into_iter()
            .map(|(_, m)| match m {
                Val::Mod(m) => ms(&m),
                other => panic!("mod list holds {other:?}"),
            })
            .collect::<Vec<_>>()
            .join(";"),
        Val::Nil => "nil".to_owned(),
        other => panic!("mod list is {other:?}"),
    }
}

fn sdl(node: &Node) -> String {
    node.sd
        .get()
        .snapshot()
        .iter()
        .map(|l| esc(l))
        .collect::<Vec<_>>()
        .join("\u{1}")
}

fn modsl(node: &Node) -> String {
    node.mods
        .get()
        .snapshot()
        .iter()
        .map(|e| {
            format!(
                "{}/{}/{}",
                if e.truthy(n!("combined")) { "C" } else { "" },
                tostr(&e.get(n!("extra"))),
                ml_table(&e.get(n!("list")))
            )
        })
        .collect::<Vec<_>>()
        .join("|")
}

fn opt_name(n: Option<Name>) -> String {
    n.map_or_else(|| "nil".to_owned(), |n| n.to_string())
}

fn node_rec(node: &Node) -> String {
    let ty = node.r#type.get();
    [
        tostr(&node_id_val(node)),
        if ty.is_empty() {
            "nil".to_owned()
        } else {
            ty.to_string()
        },
        esc(&node.dn.get()),
        opt_name(node.ascendancyName.get()),
        node.classStartIndex
            .get()
            .map_or_else(|| "nil".to_owned(), tostring_num),
        node.passivePointsGranted
            .get()
            .map_or_else(|| "nil".to_owned(), tostring_num),
        f17(node.o.get()),
        f17(node.oidx.get()),
        tostr(&node.extra.get(n!("unknown"))),
        tostr(&node.extra.get(n!("extra"))),
        esc(&node.modKey.get()),
        sdl(node),
        node.keystoneMod
            .get()
            .map_or_else(|| "nil".to_owned(), |m| ms(&m)),
    ]
    .join("\u{2}")
}

fn emit_nir(out: &mut BTreeMap<String, String>, node: &Node) {
    let id = tostr(&node_id_val(node));
    if let Some(t) = node.nodesInRadius.get() {
        for (r, _) in t.pairs() {
            let r = key_text(r);
            let m = nodesInRadiusAt(node, r.parse().unwrap()).expect("nodesInRadius[r]");
            let order: Vec<i64> = m.keys();
            let mut sorted = order.clone();
            sorted.sort_unstable();
            let j = |v: &[i64]| v.iter().map(i64::to_string).collect::<Vec<_>>().join(",");
            out.insert(format!("nir:{id}:{r}"), j(&sorted));
            out.insert(format!("niro:{id}:{r}"), j(&order));
        }
    }
    if let Some(t) = node.attributesInRadius.get() {
        out.insert(format!("air:{id}"), t.pairs().count().to_string());
    }
}

fn obj_node(v: &Val) -> Rc<Node> {
    v.obj::<Node>()
        .unwrap_or_else(|| panic!("not a node: {v:?}"))
}

fn rust_dump(ver: &str, order: Option<&[i64]>) -> BTreeMap<String, String> {
    setJewelRadiiGlobally(ver);
    let tree = match order {
        None => new_PassiveTree(Name::from(ver)),
        Some(o) => new_passive_tree_with_node_order(Name::from(ver), Some(o)),
    };
    let mut out = BTreeMap::new();
    let t: &PassiveTree = &tree;
    let pairs: Vec<(i64, NodeRef)> = t.nodes.pairs();
    out.insert(
        "order".to_owned(),
        pairs
            .iter()
            .map(|(k, _)| k.to_string())
            .collect::<Vec<_>>()
            .join(","),
    );
    let mut sorted = pairs.clone();
    sorted.sort_by_key(|(k, _)| *k);
    for (id, node) in &sorted {
        let k = id.to_string();
        out.insert(format!("node:{k}"), node_rec(node));
        out.insert(
            format!("link:{k}"),
            node.linkedId
                .get()
                .snapshot()
                .iter()
                .map(|x| tostring_num(*x))
                .collect::<Vec<_>>()
                .join(","),
        );
        out.insert(format!("ml:{k}"), ml_store(&node.modList.get()));
        out.insert(format!("mods:{k}"), modsl(node));
        out.insert(
            format!("pos:{k}"),
            format!(
                "{},{},{}",
                f17(node.x.get()),
                f17(node.y.get()),
                f17(node.angle.get())
            ),
        );
        if let Some(me) = node.masteryEffects.get() {
            let p: Vec<String> = me
                .ipairs()
                .into_iter()
                .map(|(_, e)| {
                    let stats = e.get(n!("stats"));
                    let s = stats
                        .table()
                        .map(|t| {
                            t.ipairs()
                                .into_iter()
                                .map(|(_, l)| esc(&tostr(&l)))
                                .collect::<Vec<_>>()
                                .join("\u{1}")
                        })
                        .unwrap_or_else(|| "nil".to_owned());
                    format!("{}:{s}", tostr(&e.get(n!("effect"))))
                })
                .collect();
            out.insert(format!("meff:{k}"), p.join("|"));
        }
        if node.r#type.get() == n!("Socket") {
            out.insert(
                format!("charm:{k}"),
                tostr(&node.extra.get(n!("charmSocket"))),
            );
            emit_nir(&mut out, node);
        }
        out.insert(
            format!("grp:{k}"),
            node.group.get().map_or_else(
                || "nil".to_owned(),
                |g| {
                    format!(
                        "{}|{}",
                        tostr(&g.get(n!("ascendancyName"))),
                        tostr(&g.get(n!("isAscendancyStart")))
                    )
                },
            ),
        );
    }
    for (name, node) in t.keystoneMap.pairs() {
        let node = obj_node(&node);
        out.insert(format!("km:{}", key_text(name)), tostr(&node_id_val(&node)));
        if node.nodesInRadius.get().is_some() {
            emit_nir(&mut out, &node);
        }
    }
    for (prefix, map) in [
        ("nm", &t.notableMap),
        ("am", &t.ascendancyMap),
        ("cm", &t.clusterNodeMap),
    ] {
        for (name, node) in map.pairs() {
            out.insert(
                format!("{prefix}:{}", key_text(name)),
                tostr(&node_id_val(&obj_node(&node))),
            );
        }
    }
    let mut sockets = t.sockets.keys();
    sockets.sort_unstable();
    out.insert(
        "sockets".to_owned(),
        sockets
            .iter()
            .map(i64::to_string)
            .collect::<Vec<_>>()
            .join(","),
    );
    let extra = &t.extra;
    for (name, list) in extra
        .table(n!("classNotables"))
        .expect("classNotables")
        .pairs()
    {
        let list = list.table().expect("list");
        out.insert(
            format!("cn:{}", key_text(name)),
            list.ipairs()
                .into_iter()
                .map(|(_, dn)| esc(&tostr(&dn)))
                .collect::<Vec<_>>()
                .join("|"),
        );
    }
    for (classId, class) in t.classes.pairs() {
        let cid = key_text(classId);
        out.insert(
            format!("class:{cid}"),
            format!(
                "{}|{}",
                tostr(&class.get(n!("name"))),
                tostr(&class.get(n!("startNodeId")))
            ),
        );
        for (ascId, asc) in class.get(n!("classes")).table().expect("classes").pairs() {
            out.insert(
                format!("asc:{cid}:{}", key_text(ascId)),
                format!(
                    "{}|{}|{}",
                    tostr(&asc.get(n!("name"))),
                    tostr(&asc.get(n!("startNodeId"))),
                    tostr(&asc.get(n!("id")))
                ),
            );
        }
    }
    for (name, e) in t.classNameMap.pairs() {
        out.insert(format!("cnm:{}", key_text(name)), tostr(&e));
    }
    for (name, e) in t.ascendNameMap.pairs() {
        out.insert(
            format!("anm:{}", key_text(name)),
            format!(
                "{}|{}|{}|{}",
                tostr(&e.get(n!("classId"))),
                tostr(&e.get(n!("ascendClassId"))),
                tostr(&e.get(n!("ascendClass")).get(n!("name"))),
                tostr(&e.get(n!("class")).get(n!("name")))
            ),
        );
    }
    for (prefix, map) in [
        ("ianm", &t.internalAscendNameMap),
        ("sanm", &t.secondaryAscendNameMap),
    ] {
        for (name, e) in map.pairs() {
            out.insert(
                format!("{prefix}:{}", key_text(name)),
                format!(
                    "{}|{}|{}",
                    tostr(&e.get(n!("classId"))),
                    tostr(&e.get(n!("ascendClassId"))),
                    tostr(&e.get(n!("ascendClass")).get(n!("name")))
                ),
            );
        }
    }
    if let Some(alt) = t.alternate_ascendancies.get() {
        for (k, a) in alt.pairs() {
            out.insert(format!("alt:{}", key_text(k)), tostr(&a.get(n!("id"))));
        }
    }
    let groups = extra.table(n!("groups")).expect("groups");
    let mut gids: Vec<(i64, Val)> = groups
        .pairs()
        .map(|(k, v)| match k {
            Key::I(i) => (i, v),
            other => panic!("group key {other:?}"),
        })
        .collect();
    gids.sort_by_key(|(k, _)| *k);
    for (gid, g) in gids {
        let oo = g.get(n!("oo"));
        let mut oos: Vec<i64> = oo
            .table()
            .map(|t| {
                t.pairs()
                    .map(|(k, _)| match k {
                        Key::I(i) => i,
                        other => panic!("oo key {other:?}"),
                    })
                    .collect()
            })
            .unwrap_or_default();
        oos.sort_unstable();
        let nlen = match g.get(n!("n")) {
            Val::Table(t) => t.len().to_string(),
            _ => "nil".to_owned(),
        };
        out.insert(
            format!("group:{gid}"),
            format!(
                "{}|{}|{}|{nlen}",
                tostr(&g.get(n!("ascendancyName"))),
                tostr(&g.get(n!("isAscendancyStart"))),
                oos.iter().map(i64::to_string).collect::<Vec<_>>().join(",")
            ),
        );
    }
    out.insert(
        "skillsPerOrbit".to_owned(),
        t.skillsPerOrbit
            .iter()
            .map(|x| f17(*x))
            .collect::<Vec<_>>()
            .join(","),
    );
    out.insert(
        "orbitRadii".to_owned(),
        t.orbitRadii
            .iter()
            .map(|x| f17(*x))
            .collect::<Vec<_>>()
            .join(","),
    );
    for (o, angles) in t.orbitAnglesByOrbit.iter().enumerate() {
        out.insert(
            format!("angles:{}", o + 1),
            angles.iter().map(|x| f17(*x)).collect::<Vec<_>>().join(","),
        );
    }
    out.insert("size".to_owned(), f17(extra.num(n!("size"))));
    let jr = pob_gamedata::jewelRadius();
    let mut p = vec![f17(jr.maxJewelRadius)];
    for r in &jr.jewelRadius {
        p.push(format!(
            "{}/{}/{}/{}",
            f17(r.inner),
            f17(r.outer),
            f17(r.innerSquared.unwrap_or(f64::NAN)),
            f17(r.outerSquared.unwrap_or(f64::NAN))
        ));
    }
    out.insert("jewelRadius".to_owned(), p.join(","));
    for (id, m) in t.masteryEffects.pairs() {
        let m = obj_node(&m);
        out.insert(
            format!("mastery:{}", key_text(id)),
            format!(
                "{}\u{2}{}\u{2}{}",
                node_rec(&m),
                ml_store(&m.modList.get()),
                modsl(&m)
            ),
        );
    }
    let legion = t.legion.get().expect("legion");
    for (i, node) in legion.table(n!("nodes")).expect("nodes").ipairs() {
        let node = obj_node(&node);
        out.insert(
            format!("legion:{i}"),
            format!(
                "{}\u{2}{}\u{2}{}",
                node_rec(&node),
                ml_store(&node.modList.get()),
                modsl(&node)
            ),
        );
    }
    for (i, a) in legion.table(n!("additions")).expect("additions").ipairs() {
        out.insert(
            format!("ladd:{i}"),
            format!(
                "{}|{}",
                tostr(&a.get(n!("id"))),
                esc(&tostr(&a.get(n!("dn"))))
            ),
        );
    }
    let tattoo = t.tattoo.get().expect("tattoo");
    for (name, node) in tattoo.table(n!("nodes")).expect("nodes").pairs() {
        let node = obj_node(&node);
        out.insert(
            format!("tattoo:{}", key_text(name)),
            format!(
                "{}\u{2}{}\u{2}{}",
                node_rec(&node),
                ml_store(&node.modList.get()),
                modsl(&node)
            ),
        );
    }
    for (id, dn) in tattoo.table(n!("idMap")).expect("idMap").pairs() {
        out.insert(format!("tidmap:{}", key_text(id)), esc(&tostr(&dn)));
    }
    // the Lua side's `emit`: gsub("\n", "\\n"), then a closure's text as "table"
    for v in out.values_mut() {
        *v = fn_text(v.replace('\n', "\\n"));
    }
    out
}

/// The source order of the `tree.nodes` keys in `TreeData/<ver>/tree.lua` (test-only input:
/// the dump does not record it, CCR-0024 §5). `None` for a positional node list (2_6).
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

fn parse_records(s: &str) -> BTreeMap<String, String> {
    let mut out = BTreeMap::new();
    for line in s.split('\n') {
        if line.is_empty() {
            continue;
        }
        let (k, v) = line.split_once('\t').unwrap_or((line, ""));
        let prev = out.insert(k.to_owned(), v.to_owned());
        assert!(prev.is_none(), "duplicate oracle record {k}");
    }
    out
}

fn pos_eq(a: &str, b: &str) -> bool {
    let pa: Vec<&str> = a.split(',').collect();
    let pb: Vec<&str> = b.split(',').collect();
    pa.len() == pb.len()
        && pa.iter().zip(&pb).all(|(x, y)| {
            x == y
                || match (x.parse::<f64>(), y.parse::<f64>()) {
                    (Ok(x), Ok(y)) => (x - y).abs() <= 1e-9,
                    _ => false,
                }
        })
}

/// `modLib.parseMod(line)` on the Rust side, serialised as the Lua side does (`pres`).
fn rust_pres(line: &str) -> String {
    let (list, extra) = pob_modparser::parseMod(line, false);
    let p: Vec<String> = list
        .iter()
        .flatten()
        .map(|m| fn_text(formatMod(m)))
        .collect();
    format!(
        "{}/{}/{}",
        if list.is_some() { "L" } else { "N" },
        extra.as_deref().unwrap_or("nil"),
        p.join(";")
    )
}

/// A node record mismatch that the native ModParser (units MP1-MP7) explains:
/// PoB's `ProcessStats` made at least one `modLib.parseMod` call for this node (a single or a
/// combined line; the oracle records each call with its result) whose result the Rust
/// `pob_modparser::parseMod` does not reproduce; and the record differs only in parse
/// results (`unknown`, `extra`, `modKey`, `keystoneMod`, the mod lists), never in the
/// structural fields (id, type, dn, ascendancyName, classStartIndex, passivePointsGranted,
/// o, oidx, sd). A node whose every parse call agrees must match exactly: given the same
/// parse results, ProcessStats takes the same path.
fn parser_dependent(
    lua: &BTreeMap<String, String>,
    rust: &BTreeMap<String, String>,
    rec_key: &str,
) -> bool {
    let id = rec_key.split_once(':').map_or("", |(_, id)| id);
    let (node_key, parse_key) = if rec_key.starts_with("mastery:")
        || rec_key.starts_with("legion:")
        || rec_key.starts_with("tattoo:")
    {
        (rec_key.to_owned(), format!("parse:{rec_key}"))
    } else {
        (format!("node:{id}"), format!("parse:{id}"))
    };
    let (Some(lrec), Some(rrec)) = (lua.get(&node_key), rust.get(&node_key)) else {
        return false;
    };
    // node record: 13 fields separated by \x02, then (mastery/legion/tattoo) modList and mods
    let lf: Vec<&str> = lrec.splitn(13, '\u{2}').collect();
    let rf: Vec<&str> = rrec.splitn(13, '\u{2}').collect();
    let structural = [0usize, 1, 2, 3, 4, 5, 6, 7, 11];
    if lf.len() < 13 || rf.len() < 13 || structural.iter().any(|&i| lf[i] != rf[i]) {
        return false;
    }
    let Some(calls) = lua.get(&parse_key) else {
        return false;
    };
    calls.split('\u{4}').filter(|c| !c.is_empty()).any(|c| {
        let c = c.replace("\\n", "\n");
        let (line, res) = c.split_once('\u{3}').expect("parse record");
        rust_pres(line) != res
    })
}

#[test]
fn passive_tree_acceptance() {
    let Some(src) = pob_testpaths::require(find_pob_src(), "no PoB checkout") else {
        return; // POB_ALLOW_SKIP=1
    };
    if pob_testpaths::require(init_data().then_some(()), "data/dump not found").is_none() {
        return; // POB_ALLOW_SKIP=1
    }
    let src = std::fs::canonicalize(&src).unwrap();
    let only: Option<BTreeSet<String>> = std::env::var("PT_VERSIONS")
        .ok()
        .map(|s| s.split(',').map(|v| v.trim().to_owned()).collect());
    let versions: Vec<String> = data()
        .gameVersions
        .treeVersionList
        .iter()
        .map(ToString::to_string)
        .filter(|v| only.as_ref().is_none_or(|o| o.contains(v)))
        .collect();

    // HYG3 (test audit): LuaJIT PoB (~400 MB) runs under the machine-wide oracle lock, held
    // while `pob` lives (it is declared after the lock, so it is dropped first)
    let _oracle_lock = OracleLock::acquire().unwrap_or_else(|e| panic!("oracle lock: {e}"));
    let t0 = std::time::Instant::now();
    let mut pob = JitPob::boot_oracle(&src, None).unwrap_or_else(|e| panic!("boot: {e}"));
    pob.run(LIB).unwrap_or_else(|e| panic!("{e}"));
    eprintln!("oracle boot: {:.1}s", t0.elapsed().as_secs_f64());

    let mut bad_by_kind: BTreeMap<String, usize> = BTreeMap::new();
    let mut parser_by_kind: BTreeMap<String, usize> = BTreeMap::new();
    let mut total_by_kind: BTreeMap<String, usize> = BTreeMap::new();
    let mut shown = 0usize;
    let mut nodes_total = 0usize;
    // order-sensitive records: (identical with the dump's ascending node order,
    // identical with the tree.lua source order, total)
    let mut ord: BTreeMap<&'static str, (usize, usize, usize)> = BTreeMap::new();
    for ver in &versions {
        let t = std::time::Instant::now();
        let lua = match pob.run(&format!("return __pt.dump({ver:?})")) {
            Ok(v) => match v.into_iter().next() {
                Some(Ret::Str(s)) => parse_records(&s),
                other => panic!("{ver}: oracle returned {other:?}"),
            },
            Err(e) => panic!("{ver}: oracle: {e}"),
        };
        let tl = t.elapsed().as_secs_f64();
        let t = std::time::Instant::now();
        let rust = rust_dump(ver, None);
        let tr = t.elapsed().as_secs_f64();
        let srcOrder = source_node_order(&src, ver);
        let rustSrc = srcOrder.as_deref().map(|o| rust_dump(ver, Some(o)));
        let mut ver_bad = 0usize;
        let mut ver_parser = 0usize;
        let keys: BTreeSet<&String> = lua.keys().chain(rust.keys()).collect();
        for k in keys {
            let kind = k.split(':').next().unwrap_or(k).to_owned();
            if kind == "parse" {
                // oracle-side metadata (the parse calls of each node), read by `parser_dependent`
                continue;
            }
            let (l, r) = (lua.get(k), rust.get(k));
            // pairs() orders that follow the tree.nodes order (CCR-0024 §5): reported for the
            // dump's ascending order and for the tree.lua source order
            if matches!(kind.as_str(), "order" | "niro" | "cn") {
                let name: &'static str = match kind.as_str() {
                    "order" => "tree.nodes",
                    "niro" => "nodesInRadius",
                    _ => "classNotables",
                };
                let e = ord.entry(name).or_default();
                e.2 += 1;
                if l == r {
                    e.0 += 1;
                }
                if rustSrc.as_ref().is_some_and(|m| m.get(k) == l) {
                    e.1 += 1;
                } else if kind == "order" && std::env::var_os("PT_DEBUG_ORDER").is_some() {
                    let a: Vec<&str> = l.map(|s| s.split(',').collect()).unwrap_or_default();
                    let src_rec = rustSrc.as_ref().and_then(|m| m.get(k));
                    let b: Vec<&str> = src_rec.map(|s| s.split(',').collect()).unwrap_or_default();
                    let first = a.iter().zip(&b).position(|(x, y)| x != y);
                    eprintln!(
                        "{ver} order differs: len {} vs {}, first diff at {first:?}: lua {:?} rust {:?}",
                        a.len(),
                        b.len(),
                        first.map(|i| &a[i..(i + 8).min(a.len())]),
                        first.map(|i| &b[i..(i + 8).min(b.len())])
                    );
                }
                if kind != "cn" {
                    continue;
                }
                // classNotables must hold the same names (as a multiset)
                let sorted = |v: Option<&String>| {
                    let mut x: Vec<String> = v
                        .map(|s| s.split('|').map(str::to_owned).collect())
                        .unwrap_or_default();
                    x.sort();
                    x
                };
                *total_by_kind.entry(kind.clone()).or_default() += 1;
                if sorted(l) != sorted(r) {
                    *bad_by_kind.entry(kind.clone()).or_default() += 1;
                    ver_bad += 1;
                    eprintln!("{ver} MISMATCH {k} (as a set)");
                }
                continue;
            }
            if kind == "node" {
                nodes_total += 1;
            }
            *total_by_kind.entry(kind.clone()).or_default() += 1;
            let same = match (l, r) {
                (Some(a), Some(b)) if kind == "pos" => pos_eq(a, b),
                (a, b) => a == b,
            };
            if same {
                continue;
            }
            let node_kind = matches!(
                kind.as_str(),
                "node" | "ml" | "mods" | "mastery" | "legion" | "tattoo"
            );
            if node_kind && parser_dependent(&lua, &rust, k) {
                *parser_by_kind.entry(kind.clone()).or_default() += 1;
                ver_parser += 1;
                continue;
            }
            *bad_by_kind.entry(kind.clone()).or_default() += 1;
            ver_bad += 1;
            if shown < 40 {
                shown += 1;
                let cut = |s: Option<&String>| {
                    s.map_or_else(
                        || "<missing>".to_owned(),
                        |s| s.chars().take(1500).collect(),
                    )
                };
                eprintln!("{ver} MISMATCH {k}\n  lua  {}\n  rust {}", cut(l), cut(r));
            }
        }
        eprintln!(
            "{ver}: {} records, {ver_bad} mismatches, {ver_parser} parser-dependent (lua {tl:.1}s, rust {tr:.1}s)",
            lua.len()
        );
    }
    let total: usize = total_by_kind.values().sum();
    let bad: usize = bad_by_kind.values().sum();
    let parser: usize = parser_by_kind.values().sum();
    eprintln!(
        "PT: {} versions, {nodes_total} tree nodes, {total} compared records: {} identical, {parser} parser-dependent (MP1-MP7), {bad} mismatches",
        versions.len(),
        total - bad - parser
    );
    eprintln!("  per kind (total / parser-dependent / mismatches):");
    for (k, n) in &total_by_kind {
        eprintln!(
            "    {k:10} {n:7} / {:6} / {:6}",
            parser_by_kind.get(k).copied().unwrap_or(0),
            bad_by_kind.get(k).copied().unwrap_or(0)
        );
    }
    eprintln!(
        "  pairs() orders (informational, CCR-0024 §5): identical with ascending node order / with tree.lua source order / total"
    );
    for (k, (a, b, n)) in &ord {
        eprintln!("    {k:14} {a:6} / {b:6} / {n:6}");
    }
    assert_eq!(
        bad, 0,
        "PT: {bad} mismatches outside the parser-dependent nodes"
    );
    // coverage and the parser-dependent bucket (HYG2/V2): with MP1-MP7 merged the bucket only
    // shrinks; a bad PT_VERSIONS used to compare nothing and pass
    assert!(!versions.is_empty(), "PT_VERSIONS selects no tree version");
    if only.is_none() {
        assert_eq!(versions.len(), PT_VERSIONS, "tree versions compared");
        assert!(
            nodes_total >= PT_MIN_COVERAGE.0 && total >= PT_MIN_COVERAGE.1,
            "{nodes_total} nodes / {total} records compared, expected at least {PT_MIN_COVERAGE:?}"
        );
    }
    assert!(
        parser <= PT_MAX_PARSER_DEPENDENT,
        "PT: {parser} parser-dependent records, bound {PT_MAX_PARSER_DEPENDENT}"
    );
}

/// `data.gameVersions.treeVersionList` length.
const PT_VERSIONS: usize = 41;
/// `(tree nodes, compared records)` over all 41 versions; measured by HYG2 on rust-port 746b103.
const PT_MIN_COVERAGE: (usize, usize) = (118_817, 903_303);
/// Records allowed to be parser-dependent (a parse PoB makes that the native ModParser does not
/// reproduce). HYG2 measured 48 on rust-port 746b103: all node 50692 ("Non-Unique Jewels cause
/// … Transformed to apply to Fire Damage", 3_22-3_29 and ruthless) with an ExtraJewelFunc mod,
/// which differed only in the closure's text (Lua `function: 0x…` normalised to `table` by the
/// Lua emit, Rust `FUNCTION_TEXT` was not); `fn_text` normalises both. MPFIX, rust-port 387db54:
/// 0.
const PT_MAX_PARSER_DEPENDENT: usize = 0;

/// CCR-0024 §5 evidence: PoB's own `pairs(tree.nodes)` order of a 3.10+ tree differs between
/// Lua states (the string key `root` sits in the hash part with a per-state seeded hash).
/// Boots `PT_ORDER_RUNS` (default 4) oracles and counts the distinct orders of the latest
/// tree. Opt-in (`--ignored`): it only documents PoB's behaviour.
#[test]
#[ignore = "documents PoB nondeterminism (CCR-0024 §5); run with --ignored"]
fn tree_nodes_order_varies_between_lua_states() {
    let Some(src) = pob_testpaths::require(find_pob_src(), "no PoB checkout") else {
        return; // POB_ALLOW_SKIP=1
    };
    let src = std::fs::canonicalize(&src).unwrap();
    let runs: usize = std::env::var("PT_ORDER_RUNS")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(4);
    let mut orders = BTreeSet::new();
    for _ in 0..runs {
        let _oracle_lock = OracleLock::acquire().unwrap_or_else(|e| panic!("oracle lock: {e}"));
        let mut pob = JitPob::boot_oracle(&src, None).unwrap_or_else(|e| panic!("boot: {e}"));
        let r = pob
            .run("local t = main:LoadTree(latestTreeVersion) local p = {} for id in pairs(t.nodes) do p[#p + 1] = tostring(id) end return table.concat(p, ',')")
            .unwrap_or_else(|e| panic!("{e}"));
        let Some(Ret::Str(s)) = r.into_iter().next() else {
            panic!("no order")
        };
        orders.insert(s);
    }
    eprintln!(
        "{runs} Lua states: {} distinct pairs(tree.nodes) orders",
        orders.len()
    );
}
