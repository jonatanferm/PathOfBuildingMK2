//! LUT acceptance (calc doc §11.1): `readLUT` and the abyss reads and component
//! helpers of `pob_gamedata::timeless_lut` equal PoB's under LuaJIT (interpreter,
//! `JitPob::boot_oracle`) for every jewel type × 200 seeds × all affected nodes.
//!
//! * Legion types 1-6: `data.readLUT(seed, node, type)` for every node of
//!   `data.nodeIDList` (plus three unlisted ids) × 200 seeds spread over the type's
//!   range, plus out-of-range and (Elegant Hubris) fractional seeds.
//! * Abyss types 7-10: `data.readAbyssJewelLUT(seed, socket, type)` for every socket in
//!   the file (plus an unknown one) × 200 seeds (+3 invalid), compared in `pairs()` order.
//! * Zorath (11): the whole path (every node in the file) without ascendancies (exact
//!   `pairs()` order), with all ascendancies (as a set: PoB visits them in its random
//!   string-key order), and each ascendancy alone (exact order), × 200 seeds (+3 invalid).
//! * `resolveAbyssJewelComponent` / `getAbyssJewelComponentRoll` (stat indices 1-4) for
//!   every distinct component the abyss reads produced, against the LegionPassives data.
//! * `repairLUTs()` (no `node_indices.csv`: nil after loading every legion LUT).
//!
//! Neither side writes into the PoB checkout: the Lua `io.open` refuses writes under
//! `TimelessJewelData` (PoB then skips its `.bin` cache), and the Rust side uses a
//! read-only `StdJewelFs`. Fails without a PoB checkout or `data/dump`
//! (`pob_testpaths::require`; `POB_ALLOW_SKIP=1` skips). `LUT_TYPES=1,7` limits the types.
#![allow(non_snake_case, clippy::many_single_char_names)]

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;
use std::path::PathBuf;
use std::sync::Arc;

use pob_gamedata::timeless_lut as lut;
use pob_gamedata::{DataValue, Name, Value, ValueTable};
use pob_oracle::headless::{Engine, Ret};
use pob_oracle::jit::JitPob;
use pob_oracle::oracle::{find_pob_src, find_up};

fn init_data() -> bool {
    if pob_gamedata::is_initialised() {
        return true;
    }
    let dir = std::env::var_os("POB_DUMP_DIR")
        .map(PathBuf::from)
        .or_else(|| find_up("data/dump/manifest.json").map(|p| p.parent().unwrap().to_path_buf()));
    let Some(dir) = dir else { return false };
    if dir.join("data.bin").is_file() {
        pob_gamedata::init_from_dir(&dir).unwrap_or_else(|e| panic!("data.bin: {e}"));
    } else {
        let tree = pob_gamedata::load_dump_dir(&dir).unwrap_or_else(|e| panic!("{e}"));
        pob_gamedata::init_with(
            pob_gamedata::Data::from_tree(&tree).unwrap_or_else(|e| panic!("{e}")),
        );
    }
    true
}

/// Lua run before PoB loads: refuse writes into the jewel data folder (the `.bin`
/// caches), so the oracle leaves the checkout untouched.
const PRE: &str = r#"
do
    local open = io.open
    io.open = function(path, mode)
        if mode and mode:find("w") and tostring(path):find("TimelessJewelData") then
            return nil, tostring(path) .. ": read-only (LUT test)"
        end
        return open(path, mode)
    end
end
"#;

/// Serialisers shared by every oracle chunk (numbers via `tostring`, `%.14g`).
const LIB: &str = r#"
ConPrintf = function() end
__lut = {}
function __lut.list(t)
    local parts = { tostring(#t) }
    for i = 1, #t do parts[#parts + 1] = tostring(t[i]) end
    return table.concat(parts, ",")
end
function __lut.comp(c)
    local rolls = { }
    for i, r in ipairs(c.rolls) do rolls[i] = tostring(r) end
    return tostring(c.type) .. ":" .. tostring(c.id) .. ":" .. table.concat(rolls, ",")
end
function __lut.mod(m)
    local parts = { }
    for i, c in ipairs(m) do parts[i] = __lut.comp(c) end
    return table.concat(parts, "|")
end
-- pairs() order
function __lut.nodes(t)
    local parts = { }
    for k, v in pairs(t) do parts[#parts + 1] = tostring(k) .. "=" .. __lut.mod(v) end
    return table.concat(parts, ";")
end
-- sorted by node id
function __lut.nodesSorted(t)
    local keys = { }
    for k in pairs(t) do keys[#keys + 1] = k end
    table.sort(keys)
    local parts = { }
    for _, k in ipairs(keys) do parts[#parts + 1] = tostring(k) .. "=" .. __lut.mod(t[k]) end
    return table.concat(parts, ";")
end
__lut.seen = { }
__lut.comps = { }
function __lut.collect(t)
    for _, m in pairs(t) do
        for _, c in ipairs(m) do
            local key = __lut.comp(c)
            if not __lut.seen[key] then
                __lut.seen[key] = true
                __lut.comps[#__lut.comps + 1] = c
            end
        end
    end
end
"#;

fn num(x: f64) -> String {
    if x.fract() == 0.0 && x.abs() < 1e15 {
        format!("{}", x as i64)
    } else {
        format!("{x}")
    }
}

fn lua_list(xs: &[f64]) -> String {
    let parts: Vec<String> = xs.iter().map(|x| num(*x)).collect();
    format!("{{{}}}", parts.join(","))
}

fn run_str(pob: &mut JitPob, code: &str) -> String {
    match pob.run(code) {
        Ok(v) => match v.into_iter().next() {
            Some(Ret::Str(s)) => s,
            Some(Ret::Other(s)) => s,
            None => String::new(),
        },
        Err(e) => panic!("oracle: {e}"),
    }
}

fn get(v: &Value, k: &str) -> Value {
    v.get(Name::from(k)).clone()
}

fn ser_comp(c: &Value) -> String {
    let tostring = |v: &Value| match v {
        Value::Num(x) => num(*x),
        Value::Nil => "nil".to_owned(),
        other => format!("{other:?}"),
    };
    let rolls = get(c, "rolls");
    let rolls: Vec<String> = rolls
        .as_table()
        .map(|t| t.array.iter().map(tostring).collect())
        .unwrap_or_default();
    format!(
        "{}:{}:{}",
        tostring(&get(c, "type")),
        tostring(&get(c, "id")),
        rolls.join(",")
    )
}

fn ser_mod(m: &Value) -> String {
    let parts: Vec<String> = m
        .as_table()
        .map(|t| t.array.iter().map(ser_comp).collect())
        .unwrap_or_default();
    parts.join("|")
}

fn ser_nodes(pairs: &[(i64, Value)]) -> String {
    let parts: Vec<String> = pairs
        .iter()
        .map(|(k, v)| format!("{k}={}", ser_mod(v)))
        .collect();
    parts.join(";")
}

fn ser_list(xs: &[f64]) -> String {
    let mut s = xs.len().to_string();
    for x in xs {
        s.push(',');
        s.push_str(&num(*x));
    }
    s
}

/// Compare two newline-separated result lists; returns the mismatch count.
fn compare(what: &str, lua: &str, rust: &[String], labels: &[String]) -> usize {
    let lua: Vec<&str> = lua.split('\n').collect();
    assert_eq!(
        lua.len(),
        rust.len(),
        "{what}: oracle returned {} results, Rust {}",
        lua.len(),
        rust.len()
    );
    let mut bad = 0;
    for (i, (l, r)) in lua.iter().zip(rust).enumerate() {
        if *l != r.as_str() {
            if bad < 10 {
                eprintln!(
                    "{what} MISMATCH at {}:\n  lua  {}\n  rust {}",
                    labels[i],
                    &l[..l.len().min(300)],
                    &r[..r.len().min(300)]
                );
            }
            bad += 1;
        }
    }
    let nonEmpty = rust
        .iter()
        .filter(|r| !r.is_empty() && r.as_str() != "0")
        .count();
    eprintln!(
        "{what}: {}/{} identical ({nonEmpty} non-empty)",
        rust.len() - bad,
        rust.len()
    );
    bad
}

/// `n` seeds spread evenly over `[min, max]` on the `step` grid (both ends included).
fn spread(min: f64, max: f64, step: f64, n: usize) -> Vec<f64> {
    let slots = ((max - min) / step).floor();
    let mut out: Vec<f64> = (0..n)
        .map(|k| min + step * (k as f64 * slots / (n - 1) as f64).round())
        .collect();
    out.dedup();
    out
}

fn timeless_num(t: &DataValue, k: f64) -> f64 {
    t.table().and_then(|t| t.index(k).num()).unwrap()
}

/// LegionPassives (`data.timeless.legionPassives`) as a [`Value`].
fn to_value(v: &DataValue) -> Value {
    match v {
        DataValue::Nil => Value::Nil,
        DataValue::Bool(b) => Value::Bool(*b),
        DataValue::Num(x) => Value::Num(*x),
        DataValue::Str(s) => Value::Str(s.clone()),
        DataValue::Table(t) => {
            let mut out = ValueTable::new();
            out.array = t.seq.iter().map(to_value).collect();
            for (k, v) in &t.fields {
                out.fields.push((Name::from(&k[..]), to_value(v)));
            }
            // other number keys (only `groups`) as "#<n>" fields (calc doc §9.1 convention)
            for (k, v) in &t.num {
                out.fields
                    .push((Name::from(format!("#{}", num(*k)).as_str()), to_value(v)));
            }
            assert!(t.keys.is_empty(), "unexpected keys in LegionPassives");
            Value::Table(Arc::new(out))
        }
        DataValue::Fn(_) | DataValue::Ref(_) => panic!("unexpected {v:?} in LegionPassives"),
    }
}

#[test]
#[allow(clippy::too_many_lines)]
fn lut_acceptance() {
    let Some(src) = pob_testpaths::require(find_pob_src(), "no PoB checkout") else {
        return; // POB_ALLOW_SKIP=1
    };
    if pob_testpaths::require(init_data().then_some(()), "data/dump not found").is_none() {
        return; // POB_ALLOW_SKIP=1
    }
    let src = std::fs::canonicalize(&src).unwrap();
    let src_s = src.to_string_lossy().into_owned();
    lut::set_jewel_fs(Arc::new(lut::StdJewelFs {
        script_path: src_s,
        writable: false,
    }));
    lut::set_con_printf(|_| {});
    let types: BTreeSet<u32> = std::env::var("LUT_TYPES")
        .ok()
        .map(|s| s.split(',').filter_map(|t| t.trim().parse().ok()).collect())
        .unwrap_or_else(|| (1..=11).collect());

    let t0 = std::time::Instant::now();
    let mut pob = JitPob::boot_oracle(&src, Some(PRE)).unwrap_or_else(|e| panic!("boot: {e}"));
    pob.run(LIB).unwrap_or_else(|e| panic!("{e}"));
    eprintln!("oracle boot: {:?}", t0.elapsed());

    let d = pob_gamedata::data();
    let tm = &d.timeless;
    let mut nodes: Vec<f64> = d.nodeIDList.nodes.keys().map(|k| *k as f64).collect();
    // node indices are unique (the GV loader's pairs() search relies on it)
    let idx: BTreeSet<i64> = d
        .nodeIDList
        .nodes
        .values()
        .map(|n| n.index as i64)
        .collect();
    assert_eq!(idx.len(), nodes.len(), "duplicate nodeIDList indices");
    nodes.extend([1.0, 2.0, 99999.0]);
    let mut total_bad = 0usize;
    let mut total = 0usize;

    // ---- legion: readLUT ----
    for jt in 1..=6u32 {
        if !types.contains(&jt) {
            continue;
        }
        let t = std::time::Instant::now();
        let jtf = f64::from(jt);
        let (min, max) = (
            timeless_num(&tm.timelessJewelSeedMin, jtf),
            timeless_num(&tm.timelessJewelSeedMax, jtf),
        );
        let scale = if jt == 5 { 20.0 } else { 1.0 };
        let mut seeds: Vec<f64> = spread(min, max, 1.0, 200)
            .into_iter()
            .map(|s| s * scale)
            .collect();
        assert_eq!(seeds.len(), 200);
        if jt != 1 {
            // out of range: string.byte counts from the end / past it
            seeds.extend([min * scale - scale, max * scale + scale]);
        }
        if jt == 5 {
            // Elegant Hubris seeds off the 20 grid (seed / 20 is fractional)
            seeds.extend([2010.0, 2030.0, 159_990.0]);
        }
        let code = format!(
            r"
local jt, seeds, nodes = {jt}, {}, {}
local out = {{ }}
for _, node in ipairs(nodes) do
    for _, seed in ipairs(seeds) do
        out[#out + 1] = __lut.list(data.readLUT(seed, node, jt))
    end
    if jt == 1 and data.nodeIDList[node] then
        -- free the split node (memory); the oracle never reads a node twice
        data.timelessJewelLUTs[1].data[data.nodeIDList[node].index + 1] = {{ }}
    end
end
data.timelessJewelLUTs[jt] = nil
collectgarbage('collect')
return table.concat(out, '\n')
",
            lua_list(&seeds),
            lua_list(&nodes)
        );
        let lua = run_str(&mut pob, &code);
        let mut rust = Vec::with_capacity(nodes.len() * seeds.len());
        let mut labels = Vec::with_capacity(rust.capacity());
        for &node in &nodes {
            for &seed in &seeds {
                rust.push(ser_list(&lut::readLUT(seed, node, jtf)));
                labels.push(format!("type {jt} node {} seed {}", num(node), num(seed)));
            }
        }
        lut::clear_timeless_jewel_luts(Some(jtf));
        total += rust.len();
        total_bad += compare(&format!("readLUT type {jt}"), &lua, &rust, &labels);
        eprintln!("  ({:?})", t.elapsed());
    }

    // GV out of range raises on both sides (sizes:byte from the end, then a nil seed string)
    if types.contains(&1) {
        let lua = run_str(
            &mut pob,
            "local ok = pcall(data.readLUT, 99, 6, 1) data.timelessJewelLUTs[1] = nil collectgarbage('collect') return tostring(ok)",
        );
        let prev = std::panic::take_hook();
        std::panic::set_hook(Box::new(|_| {}));
        let rust = std::panic::catch_unwind(|| lut::readLUT(99.0, 6.0, 1.0)).is_ok();
        std::panic::set_hook(prev);
        lut::clear_timeless_jewel_luts(Some(1.0));
        assert_eq!(lua, rust.to_string(), "readLUT(99, 6, 1) raising");
        eprintln!("readLUT GV out of range: both raise = {}", lua == "false");
    }

    // ---- abyss: readAbyssJewelLUT ----
    let invalid = |min: f64, max: f64| [min - 1.0, max + 1.0, min + 0.5];
    for jt in 7..=10u32 {
        if !types.contains(&jt) {
            continue;
        }
        let t = std::time::Instant::now();
        let jtf = f64::from(jt);
        let l = lut::loadAbyssJewel(jtf).expect("abyss LUT");
        let mut sockets: Vec<f64> = l.blockOffsets.keys().map(|k| *k as f64).collect();
        let header = format!(
            "{}|{}|{}|{}|{}",
            String::from_utf8_lossy(&l.format),
            num(l.seedMinimum),
            num(l.seedMaximum),
            num(l.seedIncrement),
            sockets
                .iter()
                .map(|s| num(*s))
                .collect::<Vec<_>>()
                .join(",")
        );
        let lua_header = run_str(
            &mut pob,
            &format!(
                r#"
data.readAbyssJewelLUT(100, 0, {jt})
local l = data.timelessJewelLUTs[{jt}]
local s = {{ }}
for k in pairs(l.blockOffsets) do s[#s + 1] = k end
table.sort(s)
for i, k in ipairs(s) do s[i] = tostring(k) end
return l.format .. "|" .. l.seedMinimum .. "|" .. l.seedMaximum .. "|" .. l.seedIncrement .. "|" .. table.concat(s, ",")
"#
            ),
        );
        assert_eq!(lua_header, header, "abyss type {jt} header / sockets");
        let mut seeds = spread(l.seedMinimum, l.seedMaximum, l.seedIncrement, 200);
        assert_eq!(seeds.len(), 200);
        seeds.extend(invalid(l.seedMinimum, l.seedMaximum));
        sockets.push(0.0);
        let code = format!(
            r"
local jt, seeds, sockets = {jt}, {}, {}
local out = {{ }}
for _, socket in ipairs(sockets) do
    for _, seed in ipairs(seeds) do
        local r = data.readAbyssJewelLUT(seed, socket, jt)
        __lut.collect(r)
        out[#out + 1] = __lut.nodes(r)
    end
end
data.timelessJewelLUTs[jt] = nil
collectgarbage('collect')
return table.concat(out, '\n')
",
            lua_list(&seeds),
            lua_list(&sockets)
        );
        let lua = run_str(&mut pob, &code);
        let mut rust = Vec::new();
        let mut labels = Vec::new();
        for &socket in &sockets {
            for &seed in &seeds {
                let r = lut::readAbyssJewelLUT(seed, socket, jtf, None, None);
                rust.push(ser_nodes(&r.pairs()));
                labels.push(format!(
                    "type {jt} socket {} seed {}",
                    num(socket),
                    num(seed)
                ));
            }
        }
        drop(l);
        lut::clear_timeless_jewel_luts(Some(jtf));
        total += rust.len();
        total_bad += compare(
            &format!("readAbyssJewelLUT type {jt}"),
            &lua,
            &rust,
            &labels,
        );
        eprintln!("  ({:?})", t.elapsed());
    }

    // ---- Zorath (11) ----
    if types.contains(&11) {
        let t = std::time::Instant::now();
        let l = lut::loadAbyssJewel(11.0).expect("Zorath LUT");
        let ids: Vec<f64> = l.blockOffsets.keys().map(|k| *k as f64).collect();
        let mut names: Vec<String> = l
            .ascendancyOffsets
            .as_ref()
            .unwrap()
            .iter()
            .map(|(n, _)| String::from_utf8(n.clone()).unwrap())
            .collect();
        names.sort();
        let header = format!(
            "{}|{}|{}|{}|{}|{}",
            String::from_utf8_lossy(&l.format),
            num(l.seedMinimum),
            num(l.seedMaximum),
            num(l.seedIncrement),
            ids.len(),
            names.join(",")
        );
        // the path table and its pairs() order (the Rust API takes the keys in that order)
        let lua_header = run_str(
            &mut pob,
            r#"
data.readAbyssJewelLUT(100, 0, 11, { })
local l = data.timelessJewelLUTs[11]
local ids, names = { }, { }
for k in pairs(l.blockOffsets) do ids[#ids + 1] = k end
table.sort(ids)
for k in pairs(l.ascendancyOffsets) do names[#names + 1] = k end
table.sort(names)
__lut.path = { }
for _, id in ipairs(ids) do __lut.path[id] = true end
local order = { }
for k in pairs(__lut.path) do order[#order + 1] = tostring(k) end
return l.format .. "|" .. l.seedMinimum .. "|" .. l.seedMaximum .. "|" .. l.seedIncrement .. "|" .. #ids .. "|" .. table.concat(names, ",") .. "\n" .. table.concat(order, ",")
"#,
        );
        let (lua_header, order) = lua_header.split_once('\n').unwrap();
        assert_eq!(lua_header, header, "Zorath header / nodes / ascendancies");
        let path: Vec<f64> = order.split(',').map(|s| s.parse().unwrap()).collect();
        assert_eq!(path.len(), ids.len());
        let mut seeds = spread(l.seedMinimum, l.seedMaximum, l.seedIncrement, 200);
        assert_eq!(seeds.len(), 200);
        seeds.extend(invalid(l.seedMinimum, l.seedMaximum));
        let names_lua: Vec<String> = names.iter().map(|n| format!("{n:?}")).collect();
        let code = format!(
            r"
local seeds, names = {}, {{ {} }}
local out = {{ }}
for _, seed in ipairs(seeds) do
    local r = data.readAbyssJewelLUT(seed, 0, 11, __lut.path, '<none>')
    __lut.collect(r)
    out[#out + 1] = __lut.nodes(r)
    out[#out + 1] = __lut.nodesSorted(data.readAbyssJewelLUT(seed, 0, 11, __lut.path))
    for _, name in ipairs(names) do
        local a = data.readAbyssJewelLUT(seed, 0, 11, {{ }}, name)
        __lut.collect(a)
        out[#out + 1] = __lut.nodes(a)
    end
    out[#out + 1] = __lut.nodes(data.readAbyssJewelLUT(seed, 0, 11, nil))
end
data.timelessJewelLUTs[11] = nil
collectgarbage('collect')
return table.concat(out, '\n')
",
            lua_list(&seeds),
            names_lua.join(",")
        );
        let lua = run_str(&mut pob, &code);
        let mut rust = Vec::new();
        let mut labels = Vec::new();
        for &seed in &seeds {
            let r = lut::readAbyssJewelLUT(seed, 0.0, 11.0, Some(&path), Some("<none>"));
            rust.push(ser_nodes(&r.pairs()));
            labels.push(format!("Zorath path seed {}", num(seed)));
            let full = lut::readAbyssJewelLUT(seed, 0.0, 11.0, Some(&path), None);
            let mut sorted = full.pairs();
            sorted.sort_by_key(|(k, _)| *k);
            rust.push(ser_nodes(&sorted));
            labels.push(format!("Zorath path+ascendancies seed {}", num(seed)));
            for name in &names {
                let a = lut::readAbyssJewelLUT(seed, 0.0, 11.0, Some(&[]), Some(name));
                rust.push(ser_nodes(&a.pairs()));
                labels.push(format!("Zorath {name} seed {}", num(seed)));
            }
            let none = lut::readAbyssJewelLUT(seed, 0.0, 11.0, None, None);
            rust.push(ser_nodes(&none.pairs()));
            labels.push(format!("Zorath nil path seed {}", num(seed)));
        }
        drop(l);
        lut::clear_timeless_jewel_luts(Some(11.0));
        for (i, kind) in ["path", "path+ascendancies"].iter().enumerate() {
            let n = rust
                .iter()
                .skip(i)
                .step_by(names.len() + 3)
                .filter(|r| !r.is_empty())
                .count();
            eprintln!("  Zorath {kind}: {n} non-empty of {}", seeds.len());
        }
        total += rust.len();
        total_bad += compare("readAbyssJewelLUT type 11", &lua, &rust, &labels);
        eprintln!("  ({:?})", t.elapsed());
    }

    // ---- abyss components ----
    if types.iter().any(|t| *t >= 7) {
        let t = std::time::Instant::now();
        let lua = run_str(
            &mut pob,
            r#"
local legion = LoadModule("Data/TimelessJewelData/LegionPassives")
local out = { }
for _, c in ipairs(__lut.comps) do
    local node, isNode = data.resolveAbyssJewelComponent(c, legion)
    local s = __lut.comp(c) .. "\t" .. tostring(node and node.id) .. "," .. tostring(isNode)
    if node then
        for statIndex = 1, 4 do
            local k, m, r = data.getAbyssJewelComponentRoll(c, node, statIndex)
            s = s .. "," .. tostring(k) .. "/" .. tostring(m and m.index) .. "/" .. tostring(r)
        end
    end
    out[#out + 1] = s
end
return table.concat(out, "\n")
"#,
        );
        let legion = to_value(&tm.legionPassives);
        let mut rust = Vec::new();
        let mut labels = Vec::new();
        let lines: Vec<&str> = if lua.is_empty() {
            Vec::new()
        } else {
            lua.split('\n').collect()
        };
        for line in &lines {
            let key = line.split('\t').next().unwrap();
            let mut it = key.splitn(3, ':');
            let (ty, id, rolls) = (it.next().unwrap(), it.next().unwrap(), it.next().unwrap());
            let mut c = ValueTable::new();
            let numv = |s: &str| s.parse::<f64>().map_or(Value::Nil, Value::Num);
            c.set(Name::from("type"), numv(ty));
            c.set(Name::from("id"), numv(id));
            let mut r = ValueTable::new();
            if !rolls.is_empty() {
                r.array = rolls.split(',').map(numv).collect();
            }
            c.set(Name::from("rolls"), r.into());
            let c: Value = c.into();
            let (node, isNode) = lut::resolveAbyssJewelComponent(&c, &legion);
            let tostr = |v: &Value| match v {
                Value::Nil => "nil".to_owned(),
                Value::Num(x) => num(*x),
                Value::Str(s) => s.to_string(),
                Value::Bool(b) => b.to_string(),
                other => format!("{other:?}"),
            };
            let node_id = if node.truthy() {
                tostr(&get(&node, "id"))
            } else {
                "nil".to_owned()
            };
            let mut s = format!(
                "{}\t{},{}",
                ser_comp(&c),
                node_id,
                isNode.map_or("nil".to_owned(), |b| b.to_string())
            );
            if node.truthy() {
                for statIndex in 1..=4 {
                    let (k, m, roll) =
                        lut::getAbyssJewelComponentRoll(&c, &node, f64::from(statIndex));
                    let mi = if m.truthy() {
                        get(&m, "index")
                    } else {
                        Value::Nil
                    };
                    let _ = write!(
                        s,
                        ",{}/{}/{}",
                        tostr(&k),
                        tostr(&mi),
                        roll.map_or("nil".to_owned(), num)
                    );
                }
            }
            rust.push(s);
            labels.push(key.to_owned());
        }
        assert!(!rust.is_empty(), "no abyss components collected");
        total += rust.len();
        total_bad += compare("abyss components", &lua, &rust, &labels);
        eprintln!("  ({:?})", t.elapsed());
    }

    // ---- repairLUTs (no node_indices.csv: nil) ----
    if types.len() == 11 {
        let lua = run_str(
            &mut pob,
            "local r = data.repairLUTs() for t = 1, 6 do data.timelessJewelLUTs[t] = nil end collectgarbage('collect') return tostring(r)",
        );
        let rust = lut::repairLUTs();
        lut::clear_timeless_jewel_luts(None);
        assert_eq!(lua, "nil");
        assert!(rust.is_none(), "repairLUTs returned a list");
        eprintln!("repairLUTs: nil on both sides");
    }

    let summary: BTreeMap<&str, usize> = [("results", total), ("mismatches", total_bad)].into();
    eprintln!("LUT acceptance: {summary:?} ({:?})", t0.elapsed());
    assert_eq!(total_bad, 0, "LUT results differ from PoB");
    if types.len() == 11 {
        // coverage (HYG2/V2): every jewel type x seed x node result was compared
        assert_eq!(total, LUT_RESULTS, "LUT results compared");
    }
}

/// Results compared over all 11 jewel types; measured by HYG2 on rust-port 746b103.
const LUT_RESULTS: usize = 2_380_556;
