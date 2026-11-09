//! Unit CS0 acceptance (calc doc §11.1 row CS0): CalcSetup.lua 1-474 —
//! `calcs.initModDB`, `calcs.buildModListForNode`, `calcs.buildModListForNodeList`, `wipeEnv`,
//! `applyGemMods`, `applySocketMods`, `addBestSupport` (`pob_calc::modules::calc_setup::{
//! init_mod_db, node_mods, support}`).
//!
//! **Function probes, seeded from the oracle.** PoB's sources are patched
//! (`pob_oracle::patcher`, `corpus/CS0/stages.toml`: `[[function]]` entries for the six
//! functions) and booted under LuaJIT (JIT off) with `corpus/CS0/cs0.lua` as the probe runtime:
//! its `__PROBE_FN` records every probed call's inputs and effects (first N calls per build).
//! Every build of all57 (`.parity/builds`) and `corpus/CS0/*.xml` is loaded; afterwards `wipeEnv` is driven on fresh
//! CALCULATOR envs with every `accelerate` shape. For each record the test rebuilds the inputs
//! as Rust values, runs the Rust function and compares:
//!
//! * `initModDB`: every mod (by name, in store order) and the three conditions;
//! * `buildModListForNodeList` (and through it `buildModListForNode` for every node of the
//!   list and of `env.extraRadiusNodeList`): the returned list, `explodeSources`, every node's
//!   `finalModList` (MAIN) and `grantedSkills`, every radius jewel's `data` after the finish
//!   call and `item.jewelRadiusData`. The jewel funcs are MP7's `callJewelFunc`; the func of a
//!   radius jewel is recovered from the parsed line PoB's closure came from
//!   (`modLib.parseModCache`) and parsed again by the Rust ModParser (MP7's approach; the two
//!   ExtraJewelFunc closures per CCR-0039), the default func as `"$default"`;
//! * `wipeEnv`: the size of every table it may wipe, before (seeded) and after;
//! * `applyGemMods`: the effect fields every row's key names, `itemQuality`/`globalQuality`
//!   and which rows were appended to `gemPropertyInfo`;
//! * `applySocketMods`: the mods added to `env.player.modDB` (as a multiset; a ModDB is a
//!   name-keyed hash in both);
//! * `addBestSupport`: the list after the call (which effect sits at each index) and every
//!   `superseded` flag.
//!
//! Why not the pob-state dumps of `pob_oracle probe` + ORC-X injection: nodes are dumped as
//! `$node` markers that resolve against the Rust build's spec (BM-A / PS-A are not ported, so
//! they would be empty placeholders), and closures are dumped without upvalues, so a radius
//! jewel's func cannot be rebuilt from a dump.
//!
//! Inputs: `data/dump`, the PoB checkout, `.parity/builds` (missing fails unless
//! `POB_ALLOW_SKIP=1`). Knobs: `CS0_BUILDS=stem1,stem2`, `CS0_VERBOSE=1`, `CS0_MAX_BUILDS=n`.
#![allow(non_snake_case)]
#![allow(clippy::too_many_lines, clippy::float_cmp)]

use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};
use std::sync::Once;

use pob_calc::modules::calc_setup::{
    buildModListForNodeList, initModDB,
    support::{addBestSupport, applyGemMods, applySocketMods},
    wipeEnv,
};
use pob_calc::prelude::*;
use pob_gamedata::{JewelFuncKind, JewelFuncSpec};
use pob_oracle::headless::{Engine, Ret};
use pob_oracle::jit::JitPob;
use pob_oracle::oracle::QUIET_PRELUDE;
use pob_oracle::runner::OracleLock;
use serde_json::Value as J;

const CS0_LUA: &str = include_str!("../corpus/CS0/cs0.lua");

fn init_data() -> bool {
    static INIT: Once = Once::new();
    let Some(dir) = pob_testpaths::require(pob_testpaths::dump_dir(), "data/dump (pob-dump)")
    else {
        return false;
    };
    INIT.call_once(|| {
        pob_gamedata::init_from_dir(&dir).unwrap_or_else(|e| panic!("data.bin: {e}"));
    });
    true
}

/// A LuaError panic as `Err` (other panics propagate).
fn catch<R>(f: impl FnOnce() -> R) -> Result<R, LuaError> {
    match std::panic::catch_unwind(std::panic::AssertUnwindSafe(f)) {
        Ok(r) => Ok(r),
        Err(p) => match p.downcast::<LuaError>() {
            Ok(e) => Err(*e),
            Err(other) => std::panic::resume_unwind(other),
        },
    }
}

fn quiet_lua_errors() {
    let prev = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        if info.payload().downcast_ref::<LuaError>().is_none() {
            prev(info);
        }
    }));
}

// ---- decoding (the MP7 corpus encodings) -------------------------------------------------------

/// A mod entry `{"mod": {…}, "marks": {…}}`.
fn decode_mod(e: &J) -> Mod {
    let mut m: Mod =
        serde_json::from_value(e["mod"].clone()).unwrap_or_else(|x| panic!("mod {e}: {x}"));
    if let Some(marks) = e["marks"].as_object() {
        if !marks.is_empty() {
            let mut ex = m.extra.take().map(|b| *b).unwrap_or_default();
            for (k, v) in marks {
                let v: Value = serde_json::from_value(v.clone()).unwrap();
                ex.set(Name::from(k.as_str()), v);
            }
            m.extra = Some(Box::new(ex));
        }
    }
    m
}

/// A JSON list (`[]` or the empty table's `{}`).
fn list(j: &J) -> Vec<J> {
    match j {
        J::Array(a) => a.clone(),
        J::Object(o) if o.is_empty() => Vec::new(),
        J::Null => Vec::new(),
        other => panic!("not a list: {other}"),
    }
}

fn mods(j: &J) -> Vec<Mod> {
    list(j).iter().map(decode_mod).collect()
}

fn jnum(v: &J) -> Option<f64> {
    match v {
        J::Number(n) => n.as_f64(),
        J::Object(o) => match o.get("$float").and_then(J::as_str) {
            Some("-0") => Some(-0.0),
            Some("nan") => Some(f64::NAN),
            Some("inf") => Some(f64::INFINITY),
            Some("-inf") => Some(f64::NEG_INFINITY),
            _ => None,
        },
        _ => None,
    }
}

/// Numbers equal (±0 is a host artefact on a dual-number oracle, CCR-0002: `==` ignores it).
fn num_eq(a: Option<f64>, b: Option<f64>) -> bool {
    match (a, b) {
        (Some(x), Some(y)) => x == y || (x.is_nan() && y.is_nan()),
        (None, None) => true,
        _ => false,
    }
}

fn key_text(k: Key) -> String {
    match k {
        Key::S(s) => s.as_str().to_owned(),
        Key::I(i) => i.to_string(),
        Key::B(b) => b.to_string(),
    }
}

/// The Lua `data` encoding (`encData`) against the Rust value.
fn cmp_data(lua: &J, rust: &Val, path: &str) -> Result<(), String> {
    cmp_data_o(lua, rust, path, true)
}

/// `ordered == false`: ModLists inside compare as multisets (the node iteration order differs).
fn cmp_data_o(lua: &J, rust: &Val, path: &str, ordered: bool) -> Result<(), String> {
    match (lua, rust) {
        (J::Null, Val::Nil) => Ok(()),
        (J::Bool(a), Val::Bool(b)) if a == b => Ok(()),
        (J::String(a), Val::Str(b)) if a.as_str() == &**b => Ok(()),
        (J::Object(o), Val::Obj(_)) => {
            let Some(l) = rust.obj::<ModStore>() else {
                return Err(format!("{path}: userdata is not a ModList"));
            };
            let Some(J::Array(ms)) = o.get("$ModList") else {
                return Err(format!("{path}: Lua {lua} vs a ModList"));
            };
            let lm: Vec<Mod> = ms.iter().map(decode_mod).collect();
            if ordered {
                cmp_mod_lists(&lm, &l.ipairs(), path)
            } else {
                cmp_mod_multisets(&lm, &l.ipairs(), path)
            }
        }
        (J::Object(o), Val::Table(t)) if !o.contains_key("$float") => {
            let mut ours: Vec<(String, Val)> = t.pairs().map(|(k, v)| (key_text(k), v)).collect();
            ours.sort_by(|a, b| a.0.cmp(&b.0));
            let mut lks: Vec<&String> = o.keys().collect();
            lks.sort();
            let rk: Vec<&String> = ours.iter().map(|(k, _)| k).collect();
            if lks != rk {
                return Err(format!("{path}: keys {lks:?} vs {rk:?}"));
            }
            for (k, v) in &ours {
                cmp_data_o(&o[k], v, &format!("{path}.{k}"), ordered)?;
            }
            Ok(())
        }
        (J::Object(o), Val::Data(Value::Table(_))) if !o.contains_key("$float") => {
            let v: Value =
                serde_json::from_value(lua.clone()).map_err(|e| format!("{path}: {e}"))?;
            if Val::from_value(&v).raw_eq(rust) || matches!(rust, Val::Data(d) if *d == v) {
                Ok(())
            } else {
                Err(format!("{path}: Lua {lua} vs Rust {rust:?}"))
            }
        }
        (l, Val::Num(b)) => {
            if num_eq(jnum(l), Some(*b)) {
                Ok(())
            } else {
                Err(format!("{path}: {l} vs {b}"))
            }
        }
        (l, r) => Err(format!("{path}: Lua {l} vs Rust {r:?}")),
    }
}

fn cmp_mod_lists(lua: &[Mod], rust: &[ModRef], path: &str) -> Result<(), String> {
    if lua.len() != rust.len() {
        return Err(format!(
            "{path}: {} mods vs {} (Lua {:?} / Rust {:?})",
            lua.len(),
            rust.len(),
            lua.iter().map(|m| m.name.as_str()).collect::<Vec<_>>(),
            rust.iter().map(|m| m.name.as_str()).collect::<Vec<_>>()
        ));
    }
    for (i, (a, b)) in lua.iter().zip(rust).enumerate() {
        if a != &**b {
            return Err(format!("{path}[{}]: Lua {a:?}\n   vs Rust {b:?}", i + 1));
        }
    }
    Ok(())
}

/// As [`cmp_mod_lists`], ignoring order.
fn cmp_mod_multisets(lua: &[Mod], rust: &[ModRef], path: &str) -> Result<(), String> {
    let mut a: Vec<String> = lua.iter().map(|m| format!("{m:?}")).collect();
    let mut b: Vec<String> = rust.iter().map(|m| format!("{:?}", **m)).collect();
    a.sort();
    b.sort();
    if a == b {
        Ok(())
    } else {
        let only_a: Vec<&String> = a.iter().filter(|x| !b.contains(x)).take(3).collect();
        let only_b: Vec<&String> = b.iter().filter(|x| !a.contains(x)).take(3).collect();
        Err(format!(
            "{path}: {} vs {} mods; only Lua {only_a:?}; only Rust {only_b:?}",
            a.len(),
            b.len()
        ))
    }
}

// ---- Rust-side inputs ----------------------------------------------------------------------------

fn mode_of(s: &str) -> Mode {
    match s {
        "MAIN" => Mode::MAIN,
        "CALCULATOR" => Mode::CALCULATOR,
        _ => Mode::CALCS,
    }
}

fn new_env(mode: Mode) -> EnvRef {
    let build = pob_oracle::testkit::empty_build();
    let spec = build.spec.get();
    Env::new(&build, mode, Rc::new(Override::default()), spec)
}

fn node_from(n: &J) -> NodeRef {
    let modList = ModStore::ModList(None);
    for m in mods(&n["mods"]) {
        modList.AddMod(Arc::new(m));
    }
    let ks = (!n["ks"].is_null()).then(|| Arc::new(decode_mod(&n["ks"])));
    let node = Node {
        id: n["id"].as_f64().unwrap_or(f64::NAN),
        ..Default::default()
    };
    if node.id.is_nan() {
        if let Some(s) = n["id"].as_str() {
            node.extra.set(n!("id"), s);
        }
    }
    node.r#type
        .set(Name::from(n["type"].as_str().unwrap_or("")));
    node.isTattoo.set(n["tattoo"].as_bool().unwrap_or(false));
    node.modList.set(modList);
    node.keystoneMod.set(ks);
    if n["conq"].as_bool().unwrap_or(false) {
        let c = Table::new();
        c.set(n!("id"), 1.0);
        node.conqueredBy.set(Some(c));
    }
    Rc::new(node)
}

/// The two specialModList ExtraJewelFunc entries (ModParser.lua:2621/2629) as CCR-0039 encodes
/// them (MP7's test helper).
fn extra_jewel_func(line: &str) -> Option<JewelFuncSpec> {
    let lower = pob_lua::string::lower(line);
    let mk = |key: &str, args: Vec<Value>, radius: &str| JewelFuncSpec {
        key: Name::from(key),
        kind: JewelFuncKind::Other,
        args: args.into_iter().collect(),
        r#type: Some(n!("Other")),
        radius: Some(Name::from(pob_lua::string::firstToUpper(radius).as_str())),
    };
    let p1 = "^non%-unique jewels cause increases and reductions to other damage types in a (%a+) radius to be transformed to apply to (%a+) damage$";
    if let Some(c) = pob_lua::lmatch(&lower, p1, None) {
        let s: Vec<String> = c.iter().map(|x| x.to_str().into_owned()).collect();
        return Some(mk(
            "Modules/ModParser.lua:2622-2627",
            vec![Value::from(s[1].as_str())],
            &s[0],
        ));
    }
    let p2 = "^non%-unique jewels cause small and notable passive skills in a (%a+) radius to also grant %+(%d+) to (%a+)$";
    if let Some(c) = pob_lua::lmatch(&lower, p2, None) {
        let s: Vec<String> = c.iter().map(|x| x.to_str().into_owned()).collect();
        return Some(mk(
            "Modules/ModParser.lua:2630-2634",
            vec![Value::from(s[1].as_str()), Value::from(s[2].as_str())],
            &s[0],
        ));
    }
    None
}

/// The jewel func of a parsed line, as the Rust side gets it. Mod upvalues get fresh `Arc`s, so
/// state MP7 keys by the closure's baseMod (getThreshold's LUA-ALIAS source) starts fresh per
/// build, as it does in the oracle's fresh Lua state.
fn rust_func(line: &str) -> Result<JewelFuncSpec, String> {
    if let Some(s) = extra_jewel_func(line) {
        return Ok(s);
    }
    let (mods, _extra) =
        catch(|| pob_modparser::parseModNative(line)).map_err(|e| format!("parse error: {e}"))?;
    let Some(ms) = mods else {
        return Err("no mods".to_owned());
    };
    for m in ms {
        if let Value::JewelFunc(f) = m.value.get(n!("func")) {
            let mut s = (**f).clone();
            s.args = s
                .args
                .iter()
                .map(|v| match v {
                    Value::Mod(m) => Value::Mod(Arc::new((**m).clone())),
                    o => o.clone(),
                })
                .collect();
            return Ok(s);
        }
    }
    Err("no jewel func".to_owned())
}

fn default_func() -> JewelFuncSpec {
    JewelFuncSpec {
        key: n!("$default"),
        kind: JewelFuncKind::Self_,
        args: Default::default(),
        r#type: Some(n!("Self")),
        radius: None,
    }
}

thread_local! {
    /// Every func spec ever built stays alive, so no later spec reuses an address MP7's
    /// per-baseMod state is keyed by.
    static KEEP: std::cell::RefCell<Vec<Arc<JewelFuncSpec>>> = const { std::cell::RefCell::new(Vec::new()) };
}

fn gem_by_id(id: &str) -> Option<GemDataRef> {
    let d = data();
    d.gems
        .get(&Name::from(id))
        .cloned()
        .or_else(|| d.gems.values().find(|g| g.id.as_str() == id).cloned())
}

fn ge_by_id(id: &str) -> Option<GrantedEffectRef> {
    data().skills.get(&Name::from(id)).cloned()
}

// ---- checks ------------------------------------------------------------------------------------

#[derive(Default)]
struct Tally {
    records: usize,
    ok: usize,
    skipped: usize,
    mismatches: Vec<String>,
}

#[derive(Default, Debug)]
struct Stats {
    nodes: usize,
    extra_nodes: usize,
    rads: usize,
    rad_line_funcs: usize,
    rad_default_funcs: usize,
    non_main: usize,
    explode: usize,
    granted: usize,
    gem_rows: usize,
    socket_added: usize,
    support_list_entries: usize,
    init_pre_mods: usize,
}

struct BuildCtx {
    stem: String,
    /// The radius jewels' items by id: PoB's items outlive the envs, so `jewelRadiusData` set
    /// by an earlier MAIN call is still there in a later call.
    items: HashMap<String, ItemRef>,
    stats: Stats,
    /// line → func spec (one per build, shared by every radius jewel of that line).
    funcs: HashMap<String, Arc<JewelFuncSpec>>,
    order_mismatch: usize,
}

fn check_initModDB(rec: &J) -> Result<(), String> {
    let i = &rec["in"];
    let env = new_env(Mode::CALCS);
    env.mode_buffs.set(i["buffs"].as_bool().unwrap_or(false));
    env.mode_combat.set(i["combat"].as_bool().unwrap_or(false));
    env.mode_effective
        .set(i["effective"].as_bool().unwrap_or(false));
    let modDB = ModStore::ModDB(None);
    // the mods already in the DB (CalcPerform.lua:1159 runs it on a minion DB that has some)
    for (_, v) in i["pre"].as_object().unwrap() {
        for m in mods(v) {
            modDB.AddMod(Arc::new(m));
        }
    }
    catch(|| initModDB(&env, &modDB)).map_err(|e| format!("Rust raised: {e:?}"))?;
    let o = &rec["out"];
    let lua: BTreeMap<String, Vec<Mod>> = o["mods"]
        .as_object()
        .unwrap()
        .iter()
        .map(|(k, v)| (k.clone(), mods(v)))
        .collect();
    let rnames: Vec<String> = modDB
        .mod_names()
        .iter()
        .map(|n| n.as_str().to_owned())
        .collect();
    let lnames: Vec<String> = lua.keys().cloned().collect();
    if rnames != lnames {
        return Err(format!("names {lnames:?} vs {rnames:?}"));
    }
    for (name, lm) in &lua {
        cmp_mod_lists(lm, &modDB.mods_named(Name::from(name.as_str())), name)?;
    }
    cmp_data(
        &o["conditions"],
        &Val::Table(modDB.conditions.clone()),
        "conditions",
    )
}

fn check_nodeList(rec: &J, ctx: &mut BuildCtx) -> Result<(), String> {
    let i = &rec["in"];
    let mode = mode_of(i["mode"].as_str().unwrap());
    let env = new_env(mode);
    // nodeList, in Lua's pairs order
    let nodes: Vec<NodeRef> = list(&i["nodes"]).iter().map(node_from).collect();
    // Rebuild Lua's table layout: first the copyTable(spec.allocNodes) insertion order, then the
    // nodes added later (granted passives, CalcSetup.lua:1392) in their pairs order; else the
    // pairs order itself.
    let lua_order: Vec<i64> = nodes.iter().map(|n| n.id as i64).collect();
    let by_id: HashMap<i64, NodeRef> = nodes.iter().map(|n| (n.id as i64, n.clone())).collect();
    let spec_order: Vec<i64> = list(&i["specOrder"])
        .iter()
        .filter_map(J::as_f64)
        .map(|x| x as i64)
        .collect();
    let mut tries: Vec<Vec<i64>> = Vec::new();
    {
        let mut a: Vec<i64> = spec_order
            .iter()
            .copied()
            .filter(|k| by_id.contains_key(k))
            .collect();
        a.extend(
            lua_order
                .iter()
                .copied()
                .filter(|k| !spec_order.contains(k)),
        );
        tries.push(a);
    }
    tries.push(lua_order.clone());
    let mut map = LjIntMap::new();
    let mut order_ok = false;
    for t in &tries {
        let mut m = LjIntMap::new();
        for k in t {
            m.set(*k, Some(by_id[k].clone()));
        }
        let ok = m.keys() == lua_order;
        map = m;
        if ok {
            order_ok = true;
            break;
        }
    }
    let nodeList = Shared::new(map);
    if i["isAlloc"].as_bool() == Some(true) {
        env.allocNodes.set(nodeList.clone());
    } else {
        let mut alloc = LjIntMap::new();
        for id in list(&i["allocIds"]) {
            let id = id.as_f64().unwrap() as i64;
            let n = by_id.get(&id).cloned().unwrap_or_else(|| {
                Rc::new(Node {
                    id: id as f64,
                    ..Default::default()
                })
            });
            alloc.set(id, Some(n));
        }
        env.allocNodes.set(Shared::new(alloc));
    }
    let extra: Vec<NodeRef> = list(&i["extra"]).iter().map(node_from).collect();
    let mut emap = LjIntMap::new();
    for n in &extra {
        emap.set(n.id as i64, Some(n.clone()));
    }
    let extra_order: Vec<i64> = extra.iter().map(|n| n.id as i64).collect();
    let order_ok = order_ok && emap.keys() == extra_order;
    env.extraRadiusNodeList.set(Shared::new(emap));
    // radius jewels
    let items = &mut ctx.items;
    let rads = env.radiusJewelList.get();
    for r in list(&i["rads"]) {
        let rad = Table::new();
        let func = &r["func"];
        let spec = if func["default"].as_bool() == Some(true) {
            Arc::new(default_func())
        } else if let Some(line) = func["line"].as_str() {
            match ctx.funcs.get(line) {
                Some(s) => s.clone(),
                None => {
                    let s =
                        Arc::new(rust_func(line).map_err(|e| format!("func of {line:?}: {e}"))?);
                    KEEP.with(|k| k.borrow_mut().push(s.clone()));
                    ctx.funcs.insert(line.to_owned(), s.clone());
                    s
                }
            }
        } else {
            return Err(format!(
                "radius jewel func not resolved by the oracle: {func}"
            ));
        };
        let mut nmap: LjIntMap<NodeRef> = LjIntMap::new();
        for e in list(&r["nodes"]) {
            let id = e[0].as_f64().unwrap();
            let n = Node {
                id,
                ..Default::default()
            };
            n.r#type.set(Name::from(e[1].as_str().unwrap_or("")));
            nmap.set(id as i64, Some(Rc::new(n)));
        }
        rad.set(n!("nodes"), Val::Obj(Rc::new(nmap)));
        rad.set(n!("func"), Val::Data(Value::JewelFunc(spec)));
        rad.set(
            n!("type"),
            Val::from_value(&serde_json::from_value::<Value>(r["type"].clone()).unwrap()),
        );
        rad.set(n!("nodeId"), r["nodeId"].as_f64().unwrap());
        let item = items
            .entry(r["itemId"].to_string())
            .or_insert_with(|| {
                let it = Item::default();
                it.id.set(r["itemId"].as_f64());
                Rc::new(it)
            })
            .clone();
        rad.set(n!("item"), Val::Obj(item));
        rad.set(n!("data"), Table::new());
        rads.push(rad);
    }
    if !order_ok {
        ctx.order_mismatch += 1;
    }
    let finish = i["finishJewels"].as_bool() == Some(true);
    let (modList, explode) = catch(|| buildModListForNodeList(&env, &nodeList, finish))
        .map_err(|e| format!("Rust raised: {e:?}"))?;
    let o = &rec["out"];
    let lml = mods(&o["modList"]);
    if order_ok {
        cmp_mod_lists(&lml, &modList.ipairs(), "modList")?;
    } else {
        cmp_mod_multisets(
            &lml,
            &modList.ipairs(),
            "modList (node order differs: multiset)",
        )?;
    }
    let lex = list(&o["explode"]);
    let rex: Vec<String> = explode
        .iter()
        .map(|v| match v {
            Val::Bool(b) => b.to_string(),
            Val::Obj(_) => v
                .obj::<Node>()
                .map_or("?".to_owned(), |n| tostring_num(n.id)),
            other => format!("{other:?}"),
        })
        .collect();
    let lexs: Vec<String> = lex
        .iter()
        .map(|v| match v {
            J::Bool(b) => b.to_string(),
            J::Number(n) => tostring_num(n.as_f64().unwrap()),
            other => other.to_string(),
        })
        .collect();
    let (mut lexs, mut rex) = (lexs, rex);
    if !order_ok {
        lexs.sort();
        rex.sort();
    }
    if lexs != rex {
        return Err(format!("explodeSources {lexs:?} vs {rex:?}"));
    }
    for (k, (n, per)) in nodes.iter().zip(list(&o["nodes"])).enumerate() {
        let path = format!("node {} (#{})", tostring_num(n.id), k + 1);
        match (&per["final"], n.finalModList.get()) {
            (J::Null, None) => {}
            (J::Null, Some(_)) => return Err(format!("{path}: Rust set finalModList")),
            (l, Some(r)) => cmp_mod_lists(&mods(l), &r.ipairs(), &format!("{path}.finalModList"))?,
            (_, None) => return Err(format!("{path}: no finalModList")),
        }
        let lg = list(&per["granted"]);
        let rg = n
            .grantedSkills
            .get()
            .map(|g| g.snapshot())
            .unwrap_or_default();
        if lg.len() != rg.len() {
            return Err(format!(
                "{path}: {} granted skills vs {}",
                lg.len(),
                rg.len()
            ));
        }
        for (a, b) in lg.iter().zip(&rg) {
            cmp_data(a, &Val::Table(b.clone()), &format!("{path}.grantedSkills"))?;
        }
    }
    for (n, per) in extra.iter().zip(list(&o["extra"])) {
        let path = format!("extra node {}", tostring_num(n.id));
        match (&per["final"], n.finalModList.get()) {
            (J::Null, None) => {}
            (l, Some(r)) if !l.is_null() => {
                cmp_mod_lists(&mods(l), &r.ipairs(), &format!("{path}.finalModList"))?;
            }
            (l, r) => return Err(format!("{path}: finalModList {l} vs {}", r.is_some())),
        }
    }
    for (k, (rad, ro)) in rads.snapshot().iter().zip(list(&o["rads"])).enumerate() {
        let path = format!("rad #{} ({})", k + 1, tostring_num(rad.num(n!("nodeId"))));
        cmp_data_o(
            &ro["data"],
            &rad.get(n!("data")),
            &format!("{path}.data"),
            order_ok,
        )?;
        let item = rad.get(n!("item")).obj::<Item>().unwrap();
        let jrd = item.jewelRadiusData.get();
        match (&ro["jrd"], jrd) {
            (J::Null, None) => {}
            (J::Bool(b), Some(t)) => {
                let d = t.get(rad.num(n!("nodeId")));
                let same = matches!(d, Val::Table(ref x) if rad.table(n!("data")).is_some_and(|y| x.ptr_eq(&y)));
                if same != *b {
                    return Err(format!(
                        "{path}: jewelRadiusData[nodeId] == rad.data: Lua {b}, Rust {same}"
                    ));
                }
            }
            (l, r) => return Err(format!("{path}: jewelRadiusData {l} vs {}", r.is_some())),
        }
    }
    Ok(())
}

/// The size fields of the wipeEnv records.
const WIPE_STORES: [&str; 3] = ["modDB", "enemyDB", "itemModDB"];

fn check_wipeEnv(rec: &J) -> Result<(), String> {
    let i = &rec["in"];
    let sz = &i["sizes"];
    let env = new_env(Mode::CALCULATOR);
    let n = |k: &str| jnum(&sz[k]).unwrap_or(0.0) as usize;
    let fill_store = |db: &ModDB, s: &J| {
        let s = list(s);
        for k in 0..(jnum(&s[0]).unwrap() as usize) {
            db.NewMod(
                Name::from(format!("CS0Mod{k}").as_str()),
                ModType::BASE,
                1.0,
                "CS0",
                0,
                0,
                &[],
            );
        }
        for k in 0..(jnum(&s[1]).unwrap() as usize) {
            db.conditions
                .set(Name::from(format!("C{k}").as_str()), true);
        }
        for k in 0..(jnum(&s[2]).unwrap() as usize) {
            db.multipliers
                .set(Name::from(format!("M{k}").as_str()), 1.0);
        }
    };
    fill_store(&env.modDB.get(), &sz["modDB"]);
    fill_store(&env.enemyDB.get(), &sz["enemyDB"]);
    fill_store(&env.itemModDB.get(), &sz["itemModDB"]);
    if !sz["minionDB"].is_null() {
        let m = Actor::new();
        fill_store(&m.modDB.get(), &sz["minionDB"]);
        env.minion.set(Some(m));
    }
    let node = |id: usize| {
        Rc::new(Node {
            id: id as f64,
            ..Default::default()
        })
    };
    env.allocNodes.get().with_mut(|m| {
        for k in 1..=n("allocNodes") {
            m.set(k as i64, Some(node(k)));
        }
    });
    env.grantedPassives.get().with_mut(|m| {
        for k in 1..=n("grantedPassives") {
            m.set(k as i64, Some(true));
        }
    });
    for _ in 0..n("grantedSkillsNodes") {
        env.grantedSkillsNodes.get().push(Table::new());
    }
    for _ in 0..n("radiusJewelList") {
        env.radiusJewelList.get().push(Table::new());
    }
    env.extraRadiusNodeList.get().with_mut(|m| {
        for k in 1..=n("extraRadiusNodeList") {
            m.set(k as i64, Some(node(k)));
        }
    });
    env.player.itemList.with_mut(|m| {
        for k in 0..n("itemList") {
            m.insert(
                Name::from(format!("Slot {k}").as_str()),
                Rc::new(Item::default()),
            );
        }
    });
    for _ in 0..n("grantedSkillsItems") {
        env.grantedSkillsItems.get().push(Table::new());
    }
    env.flasks.get().with_mut(|m| {
        for _ in 0..n("flasks") {
            m.set(&Rc::new(Item::default()), Some(true));
        }
    });
    env.tinctures.get().with_mut(|m| {
        for _ in 0..n("tinctures") {
            m.set(&Rc::new(Item::default()), Some(true));
        }
    });
    let opt_list = |k: &str, f: &Field<Option<ModList>>| {
        if !sz[k].is_null() {
            let l = ModStore::ModList(None);
            for j in 0..n(k) {
                l.NewMod(
                    Name::from(format!("CS0Mod{j}").as_str()),
                    ModType::BASE,
                    1.0,
                    "CS0",
                    0,
                    0,
                    &[],
                );
            }
            f.set(Some(l));
        }
    };
    opt_list("aegisModList", &env.aegisModList);
    opt_list("theIronMass", &env.theIronMass);
    opt_list("weaponModList1", &env.weaponModList1);
    for _ in 0..n("requirementsTableItems") {
        env.requirementsTableItems.get().push(Table::new());
    }
    for _ in 0..n("requirementsTableGems") {
        env.requirementsTableGems.get().push(Table::new());
    }
    let ge = data().skills.values().next().unwrap().clone();
    let mk_skill = || {
        ActiveSkill::new(
            SkillEffect::new(ge.clone(), 1.0, 0.0),
            Shared::new(Vec::new()),
            Weak::new(),
        )
    };
    for _ in 0..n("activeSkillList") {
        env.player.activeSkillList.get().push(mk_skill());
    }
    for _ in 0..n("auxSkillList") {
        env.auxSkillList.get().push(mk_skill());
    }
    let a = &i["accelerate"];
    let accel = Accelerate {
        nodeAlloc: a["nodeAlloc"].as_bool().unwrap(),
        requirementsItems: a["requirementsItems"].as_bool().unwrap(),
        requirementsGems: a["requirementsGems"].as_bool().unwrap(),
        skills: a["skills"].as_bool().unwrap(),
        everything: a["everything"].as_bool().unwrap(),
    };
    catch(|| wipeEnv(&env, &accel)).map_err(|e| format!("Rust raised: {e:?}"))?;
    // sizes after, in the record's shape
    let store = |db: &ModDB| {
        serde_json::json!([
            db.mod_names().len(),
            db.conditions.pairs().count(),
            db.multipliers.pairs().count()
        ])
    };
    let mut ours = serde_json::Map::new();
    for k in WIPE_STORES {
        let db = match k {
            "modDB" => env.modDB.get(),
            "enemyDB" => env.enemyDB.get(),
            _ => env.itemModDB.get(),
        };
        ours.insert(k.to_owned(), store(&db));
    }
    ours.insert(
        "minionDB".to_owned(),
        env.minion.get().map_or(J::Null, |m| store(&m.modDB.get())),
    );
    let num = |x: usize| J::from(x);
    ours.insert(
        "allocNodes".into(),
        num(env.allocNodes.get().with(|m| m.len())),
    );
    ours.insert(
        "grantedPassives".into(),
        num(env.grantedPassives.get().with(|m| m.len())),
    );
    ours.insert(
        "grantedSkillsNodes".into(),
        num(env.grantedSkillsNodes.get().len()),
    );
    ours.insert(
        "radiusJewelList".into(),
        num(env.radiusJewelList.get().len()),
    );
    ours.insert(
        "extraRadiusNodeList".into(),
        num(env.extraRadiusNodeList.get().with(|m| m.len())),
    );
    ours.insert(
        "itemList".into(),
        num(env.player.itemList.with(|m| m.len())),
    );
    ours.insert(
        "grantedSkillsItems".into(),
        num(env.grantedSkillsItems.get().len()),
    );
    ours.insert("flasks".into(), num(env.flasks.get().with(|m| m.len())));
    ours.insert(
        "tinctures".into(),
        num(env.tinctures.get().with(|m| m.len())),
    );
    for (k, f) in [
        ("aegisModList", &env.aegisModList),
        ("theIronMass", &env.theIronMass),
        ("weaponModList1", &env.weaponModList1),
    ] {
        ours.insert(k.into(), f.get().map_or(J::Null, |l| num(l.len())));
    }
    ours.insert(
        "requirementsTableItems".into(),
        num(env.requirementsTableItems.get().len()),
    );
    ours.insert(
        "requirementsTableGems".into(),
        num(env.requirementsTableGems.get().len()),
    );
    ours.insert(
        "activeSkillList".into(),
        num(env.player.activeSkillList.get().len()),
    );
    ours.insert("auxSkillList".into(), num(env.auxSkillList.get().len()));
    let ours = J::Object(ours);
    let lua = &rec["out"];
    for (k, v) in lua.as_object().unwrap() {
        let l = v.as_array().map_or_else(|| vec![v.clone()], Clone::clone);
        let r = ours[k]
            .as_array()
            .map_or_else(|| vec![ours[k].clone()], Clone::clone);
        let ln: Vec<Option<f64>> = l.iter().map(jnum).collect();
        let rn: Vec<Option<f64>> = r.iter().map(jnum).collect();
        if ln != rn {
            return Err(format!("{k}: after {v} vs Rust {}", ours[k]));
        }
    }
    Ok(())
}

/// `effect[k]` as the record encodes it.
fn effect_field(e: &SkillEffect, k: &str) -> Option<f64> {
    match k {
        "level" => Some(e.level.get()),
        "quality" => Some(e.quality.get()),
        "globalQuality" => e.globalQuality.get(),
        "itemQuality" => e.itemQuality.get(),
        "supportQuality" => e.supportQuality.get(),
        _ => e.extra.opt_num(k),
    }
}

fn effect_from(ej: &J, ge: GrantedEffectRef) -> SkillEffectRef {
    let e = SkillEffect::new(
        ge,
        jnum(&ej["level"]).unwrap_or(0.0),
        jnum(&ej["quality"]).unwrap_or(0.0),
    );
    for (k, v) in ej.as_object().unwrap() {
        let x = jnum(v);
        match k.as_str() {
            "level" | "quality" => {}
            "globalQuality" => e.globalQuality.set(x),
            "itemQuality" => e.itemQuality.set(x),
            "supportQuality" => e.supportQuality.set(x),
            other => {
                if let Some(x) = x {
                    e.extra.set(other, x);
                }
            }
        }
    }
    e
}

fn check_applyGemMods(rec: &J) -> Result<(), String> {
    let i = &rec["in"];
    let Some(ge) = ge_by_id(i["ge"].as_str().unwrap()) else {
        return Err(format!(
            "grantedEffect {} not in data (not replayed)",
            i["ge"]
        ));
    };
    let effect = effect_from(&i["effect"], ge);
    if let Some(g) = i["gem"].as_str() {
        let Some(gem) = gem_by_id(g) else {
            return Err(format!("gem {g} not in data (not replayed)"));
        };
        effect.gemData.set(Some(gem));
    }
    if let Some(n) = jnum(&i["gpi"]) {
        let t = Table::new();
        for _ in 0..(n as usize) {
            t.push(Table::new());
        }
        effect.gemPropertyInfo.set(Some(t));
    }
    let rows: Vec<TabRow> = list(&i["rows"])
        .iter()
        .map(|r| TabRow {
            value: serde_json::from_value(r["value"].clone()).unwrap(),
            r#mod: Arc::new(decode_mod(&r["mod"])),
        })
        .collect();
    catch(|| applyGemMods(&effect, &rows)).map_err(|e| format!("Rust raised: {e:?}"))?;
    let o = &rec["out"];
    for (k, v) in o["effect"].as_object().unwrap() {
        if !num_eq(jnum(v), effect_field(&effect, k)) {
            return Err(format!("effect.{k}: {v} vs {:?}", effect_field(&effect, k)));
        }
    }
    let gpi = effect.gemPropertyInfo.get();
    if !num_eq(jnum(&o["gpi"]), gpi.as_ref().map(|t| t.len() as f64)) {
        return Err(format!(
            "#gemPropertyInfo {} vs {:?}",
            o["gpi"],
            gpi.map(|t| t.len())
        ));
    }
    let pre = jnum(&i["gpi"]).unwrap_or(0.0) as usize;
    let appended: Vec<f64> = gpi
        .map(|t| {
            t.ipairs()
                .into_iter()
                .skip(pre)
                .map(|(_, row)| match row.get(n!("mod")) {
                    Val::Mod(m) => rows
                        .iter()
                        .position(|r| Arc::ptr_eq(&r.r#mod, &m))
                        .map_or(0.0, |p| (p + 1) as f64),
                    _ => -1.0,
                })
                .collect()
        })
        .unwrap_or_default();
    let lap: Vec<f64> = list(&o["appended"]).iter().filter_map(jnum).collect();
    if lap != appended {
        return Err(format!("gemPropertyInfo rows {lap:?} vs {appended:?}"));
    }
    Ok(())
}

fn check_applySocketMods(rec: &J) -> Result<(), String> {
    let i = &rec["in"];
    let Some(gem) = gem_by_id(i["gem"].as_str().unwrap()) else {
        return Err(format!("gem {} not in data (not replayed)", i["gem"]));
    };
    let env = new_env(Mode::CALCS);
    let db = env.modDB.get();
    let props = mods(&i["props"]);
    for m in &props {
        db.AddMod(Arc::new(m.clone()));
    }
    for (k, v) in i["conditions"].as_object().unwrap() {
        if let Some(b) = v.as_bool() {
            db.conditions.set(k.as_str(), b);
        } else if let Some(x) = jnum(v) {
            db.conditions.set(k.as_str(), x);
        }
    }
    for (k, v) in i["multipliers"].as_object().unwrap() {
        if let Some(x) = jnum(v) {
            db.multipliers.set(k.as_str(), x);
        }
    }
    if i["sameDB"].as_bool() == Some(true) {
        env.player.modDB.set(db.clone());
    }
    let cfg = ModCfg::new();
    for (k, v) in i["groupCfg"].as_object().unwrap() {
        let s = v.as_str().map(Name::from);
        let x = jnum(v);
        match k.as_str() {
            "slotName" => cfg.slotName.set(s),
            "socketColor" => cfg.socketColor.set(s),
            "socketNum" => cfg.socketNum.set(x),
            "strengthGems" => cfg.strengthGems.set(x),
            "dexterityGems" => cfg.dexterityGems.set(x),
            "intelligenceGems" => cfg.intelligenceGems.set(x),
            "skillName" => cfg.skillName.set(s),
            "flags" => cfg.flags.set(x.unwrap_or(0.0) as u32),
            "keywordFlags" => cfg.keywordFlags.set(x.unwrap_or(0.0) as u32),
            other => {
                if let Some(s) = v.as_str() {
                    cfg.extra.set(other, s);
                } else if let Some(x) = x {
                    cfg.extra.set(other, x);
                } else if let Some(b) = v.as_bool() {
                    cfg.extra.set(other, b);
                }
            }
        }
    }
    let socketNum = jnum(&i["socketNum"]).unwrap();
    let modSource = i["modSource"].as_str().map(str::to_owned);
    let before: HashMap<Name, usize> = env
        .player
        .modDB
        .get()
        .mod_names()
        .into_iter()
        .map(|n| (n, env.player.modDB.get().mods_named(n).len()))
        .collect();
    catch(|| applySocketMods(&env, &gem, &cfg, socketNum, modSource.as_deref()))
        .map_err(|e| format!("Rust raised: {e:?}"))?;
    let pdb = env.player.modDB.get();
    let mut added: Vec<ModRef> = Vec::new();
    for n in pdb.mod_names() {
        let all = pdb.mods_named(n);
        added.extend(all.into_iter().skip(before.get(&n).copied().unwrap_or(0)));
    }
    cmp_mod_multisets(&mods(&rec["out"]["added"]), &added, "added")
}

fn check_addBestSupport(rec: &J) -> Result<(), String> {
    let i = &rec["in"];
    let mk = |j: &J| -> Result<SkillEffectRef, String> {
        let Some(ge) = ge_by_id(j["ge"].as_str().unwrap()) else {
            return Err(format!(
                "grantedEffect {} not in data (not replayed)",
                j["ge"]
            ));
        };
        let e = SkillEffect::new(
            ge,
            jnum(&j["level"]).unwrap_or(0.0),
            jnum(&j["quality"]).unwrap_or(0.0),
        );
        e.superseded.set(j["superseded"].as_bool().unwrap_or(false));
        Ok(e)
    };
    let support = mk(&i["support"])?;
    let before: Vec<SkillEffectRef> = list(&i["list"]).iter().map(mk).collect::<Result<_, _>>()?;
    let applied = Shared::new(before.clone());
    let mode = mode_of(i["mode"].as_str().unwrap());
    catch(|| addBestSupport(&support, &applied, mode))
        .map_err(|e| format!("Rust raised: {e:?}"))?;
    let o = &rec["out"];
    let ours: Vec<f64> = applied
        .snapshot()
        .iter()
        .map(|e| {
            if Rc::ptr_eq(e, &support) {
                0.0
            } else {
                before
                    .iter()
                    .position(|b| Rc::ptr_eq(b, e))
                    .map_or(-1.0, |p| (p + 1) as f64)
            }
        })
        .collect();
    let lua: Vec<f64> = list(&o["list"]).iter().filter_map(jnum).collect();
    if lua != ours {
        return Err(format!("list {lua:?} vs {ours:?}"));
    }
    if o["supportSuperseded"].as_bool().unwrap_or(false) != support.superseded.get() {
        return Err(format!(
            "supportEffect.superseded {} vs {}",
            o["supportSuperseded"],
            support.superseded.get()
        ));
    }
    for (k, (l, b)) in list(&o["superseded"]).iter().zip(&before).enumerate() {
        if l.as_bool().unwrap_or(false) != b.superseded.get() {
            return Err(format!(
                "list[{}].superseded {l} vs {}",
                k + 1,
                b.superseded.get()
            ));
        }
    }
    Ok(())
}

fn record_stats(id: &str, rec: &J, st: &mut Stats) {
    let i = &rec["in"];
    let o = &rec["out"];
    match id {
        "buildModListForNodeList" => {
            st.nodes += list(&i["nodes"]).len();
            st.extra_nodes += list(&i["extra"]).len();
            for r in list(&i["rads"]) {
                st.rads += 1;
                if r["func"]["line"].is_string() {
                    st.rad_line_funcs += 1;
                } else if r["func"]["default"].as_bool() == Some(true) {
                    st.rad_default_funcs += 1;
                }
            }
            if i["mode"].as_str() != Some("MAIN") {
                st.non_main += 1;
            }
            st.explode += list(&o["explode"]).len();
            st.granted += list(&o["nodes"])
                .iter()
                .map(|n| list(&n["granted"]).len())
                .sum::<usize>();
        }
        "applyGemMods" => st.gem_rows += list(&i["rows"]).len(),
        "applySocketMods" => st.socket_added += list(&o["added"]).len(),
        "addBestSupport" => st.support_list_entries += list(&i["list"]).len(),
        "initModDB" => st.init_pre_mods += usize::from(jnum(&i["preMods"]) != Some(0.0)),
        _ => {}
    }
}

// ---- driver ------------------------------------------------------------------------------------

fn builds(parity: &Path) -> Vec<PathBuf> {
    let mut v: Vec<PathBuf> = std::fs::read_dir(parity.join("builds"))
        .map(|r| {
            r.filter_map(Result::ok)
                .map(|e| e.path())
                .filter(|p| p.extension().is_some_and(|x| x == "xml"))
                .collect()
        })
        .unwrap_or_default();
    v.sort();
    // synthetic builds of this unit: an all57 build with the SocketProperty lines
    // (ModParser.lua:3284-3289) on its body armour, which no all57 item has
    let own = Path::new(env!("CARGO_MANIFEST_DIR")).join("corpus/CS0");
    let mut synth: Vec<PathBuf> = std::fs::read_dir(&own)
        .map(|r| {
            r.filter_map(Result::ok)
                .map(|e| e.path())
                .filter(|p| p.extension().is_some_and(|x| x == "xml"))
                .collect()
        })
        .unwrap_or_default();
    synth.sort();
    v.extend(synth);
    if let Ok(list) = std::env::var("CS0_BUILDS") {
        let want: Vec<&str> = list.split(',').map(str::trim).collect();
        v.retain(|p| {
            p.file_stem()
                .is_some_and(|s| want.contains(&s.to_string_lossy().as_ref()))
        });
    }
    if let Some(n) = std::env::var("CS0_MAX_BUILDS")
        .ok()
        .and_then(|s| s.parse::<usize>().ok())
    {
        v.truncate(n);
    }
    v
}

/// PoB's side for one build: boot the patched copy with the CS0 probe runtime, load, drive.
fn oracle_records(src: &Path, xml: &Path, stem: &str) -> Result<J, String> {
    let text = std::fs::read_to_string(xml).map_err(|e| e.to_string())?;
    let json = {
        let _lock = OracleLock::acquire().map_err(|e| format!("oracle lock: {e}"))?;
        let pre = format!("{QUIET_PRELUDE}\n{CS0_LUA}");
        let mut pob = JitPob::boot_oracle(src, Some(&pre)).map_err(|e| format!("boot: {e}"))?;
        pob.load_build_xml(&text, stem)
            .map_err(|e| format!("load: {e}"))?;
        match pob
            .run("__cs0.drive(build) return __cs0.result()")
            .map_err(|e| format!("drive: {e}"))?
            .first()
        {
            Some(Ret::Str(s)) => s.clone(),
            other => return Err(format!("result: {other:?}")),
        }
    };
    serde_json::from_str(&json).map_err(|e| format!("json: {e}"))
}

#[test]
fn cs0_function_probes() {
    if !init_data() {
        return;
    }
    let Some(pob_src) = pob_testpaths::require(
        pob_testpaths::pob_src(),
        "PoB checkout (.PathOfBuilding/src)",
    ) else {
        return;
    };
    let Some(parity) = pob_testpaths::require(pob_testpaths::parity_dir(), ".parity") else {
        return;
    };
    let corpus = builds(&parity);
    if pob_testpaths::require((!corpus.is_empty()).then_some(()), ".parity/builds/*.xml").is_none()
    {
        return;
    }
    quiet_lua_errors();
    let verbose = std::env::var_os("CS0_VERBOSE").is_some();
    let stages = Path::new(env!("CARGO_MANIFEST_DIR")).join("corpus/CS0/stages.toml");
    let root = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("oracle-src");
    let patched = pob_oracle::patcher::patch(&pob_src, Some(&stages), &root)
        .unwrap_or_else(|e| panic!("patch: {e:?}"));
    let ids = [
        "initModDB",
        "buildModListForNodeList",
        "wipeEnv",
        "applyGemMods",
        "applySocketMods",
        "addBestSupport",
    ];
    let mut tally: BTreeMap<&str, Tally> = ids.iter().map(|k| (*k, Tally::default())).collect();
    let mut calls: BTreeMap<String, f64> = BTreeMap::new();
    let mut failed_builds = Vec::new();
    let mut order_mismatch = 0;
    let mut stats = Stats::default();
    let t0 = std::time::Instant::now();
    for xml in &corpus {
        let stem = xml.file_stem().unwrap().to_string_lossy().into_owned();
        let doc = match oracle_records(&patched.src, xml, &stem) {
            Ok(d) => d,
            Err(e) => {
                failed_builds.push(format!("{stem}: {e}"));
                continue;
            }
        };
        for (k, v) in doc["counts"].as_object().unwrap() {
            *calls.entry(k.clone()).or_default() += jnum(v).unwrap_or(0.0);
        }
        let mut ctx = BuildCtx {
            stem: stem.clone(),
            items: HashMap::new(),
            stats: Stats::default(),
            funcs: HashMap::new(),
            order_mismatch: 0,
        };
        for rec in list(&doc["records"]) {
            let id = rec["id"].as_str().unwrap().to_owned();
            record_stats(&id, &rec, &mut ctx.stats);
            let r = match id.as_str() {
                "initModDB" => check_initModDB(&rec),
                "buildModListForNodeList" => check_nodeList(&rec, &mut ctx),
                "wipeEnv" => check_wipeEnv(&rec),
                "applyGemMods" => check_applyGemMods(&rec),
                "applySocketMods" => check_applySocketMods(&rec),
                "addBestSupport" => check_addBestSupport(&rec),
                other => panic!("unknown record {other}"),
            };
            let t = tally.get_mut(id.as_str()).unwrap();
            t.records += 1;
            match r {
                Ok(()) => t.ok += 1,
                Err(e) if e.ends_with("(not replayed)") => t.skipped += 1,
                Err(e) => t
                    .mismatches
                    .push(format!("{}: {id}#{}: {e}", ctx.stem, rec["n"])),
            }
        }
        order_mismatch += ctx.order_mismatch;
        let st = &ctx.stats;
        stats.nodes += st.nodes;
        stats.extra_nodes += st.extra_nodes;
        stats.rads += st.rads;
        stats.rad_line_funcs += st.rad_line_funcs;
        stats.rad_default_funcs += st.rad_default_funcs;
        stats.non_main += st.non_main;
        stats.explode += st.explode;
        stats.granted += st.granted;
        stats.gem_rows += st.gem_rows;
        stats.socket_added += st.socket_added;
        stats.support_list_entries += st.support_list_entries;
        stats.init_pre_mods += st.init_pre_mods;
        if verbose {
            eprintln!("{stem}: done ({:.0}s)", t0.elapsed().as_secs_f64());
        }
    }
    eprintln!(
        "CS0: {} builds ({} failed in the oracle) in {:.0}s; PoB call counts {calls:?}",
        corpus.len(),
        failed_builds.len(),
        t0.elapsed().as_secs_f64()
    );
    for f in &failed_builds {
        eprintln!("  oracle failure: {f}");
    }
    let mut total_bad = 0;
    for (id, t) in &tally {
        eprintln!(
            "  {id}: {}/{} records identical ({} not replayable)",
            t.ok,
            t.records - t.skipped,
            t.skipped
        );
        total_bad += t.mismatches.len();
        for m in t
            .mismatches
            .iter()
            .take(if verbose { usize::MAX } else { 8 })
        {
            eprintln!("    MISMATCH {m}");
        }
    }
    eprintln!("  coverage of the replayed records: {stats:?}");
    eprintln!(
        "  nodeList records whose LjIntMap order differed from Lua's pairs order: {order_mismatch}"
    );
    assert!(
        failed_builds.is_empty(),
        "oracle failures: {failed_builds:?}"
    );
    for (id, t) in &tally {
        assert!(t.records > 0, "{id}: no records");
    }
    assert_eq!(total_bad, 0, "CS0 mismatches");
}
