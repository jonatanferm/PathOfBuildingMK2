//! Unit CS1 acceptance (calc doc §11.1 row CS1, §9.4 L4): initEnv segment CS1
//! (Modules/CalcSetup.lua 488-799, `pob_calc::modules::calc_setup::init_env_base::seg`) and
//! `pantheon.applySoulMod` (Modules/PantheonTools.lua), L4 `initEnv:488#k` → `initEnv:800#k`.
//!
//! **Oracle side (one PoB process per build).** PoB's sources are patched with the two line
//! probes of `corpus/CS1/stages.toml` (the `initEnv:488` / `initEnv:800` probes of
//! docs/port/stages.toml) and booted under LuaJIT (JIT off) with the standard probe runtime
//! (`runner::probe_prelude`: state_dump.lua + probe.lua, provenance off) plus
//! `corpus/CS1/cs1.lua`, which forwards the probes of selected initEnv calls only (the first two
//! calls of each (mode, specEnv shape) during the build load, and every call of
//! `__cs1.drive(build)`: addNodes / removeNodes / conditions overrides, cached-DB and env
//! specEnvs with and without accelerate flags, bandit / pantheon / resistance-penalty /
//! enemy-level / character-level / buff-mode variants). For each forwarded call `k` the runtime
//! writes the pob-state dumps `state/initEnv:488#k.json` and `state/initEnv:800#k.json`, and
//! cs1.lua writes `model/model#k.json`: the part of the build model the segment reads, as a
//! pob-state document.
//!
//! **Rust side (ORC-X testkit).** The build model is rebuilt from `model#k` (BM-A's XML loader
//! is not ported, so `testkit::load_build` would give an empty model): character level, the
//! config / calcs inputs, the config and party mod lists, a `PassiveSpec` whose allocated nodes
//! are spec overlays (`Node.tree`) of tree nodes carrying PoB's own node mod lists, the
//! class base stats and keystone map; and the call's `override`. Then `initEnv:488#k` is
//! injected onto that model (`inject::inject`), the Rust segment runs on the injected locals
//! (`probe::run_segment`), and `testkit::diff_against` compares the env and the locals with
//! `initEnv:800#k` (strict, no nondeterminism record: both dumps come from one process).
//!
//! The node-id map `spec.allocNodes` must iterate in PoB's `pairs` order (it is copied into
//! `env.allocNodes`, whose order is the order of `env.initialNodeModDB`). The rebuilt map is
//! filled so that its LuaJIT order equals the recorded one; a hit where no insertion order
//! reproduces it is reported (`order`) and, if it differs, counted as a failure.
//!
//! Inputs: `data/dump`, the PoB checkout, `.parity/builds` (missing inputs skip unless
//! `POB_ORACLE_REQUIRE=1`). The oracle side is cached per build in
//! `$CARGO_TARGET_TMPDIR/CS1/<stem>/` (delete to refresh). Knobs: `CS1_BUILDS=stem1,stem2`,
//! `CS1_MAX_BUILDS=n`, `CS1_VERBOSE=1`.
#![allow(
    non_snake_case,
    clippy::too_many_lines,
    clippy::explicit_auto_deref,
    clippy::single_match_else
)]

use std::path::{Path, PathBuf};

use pob_calc::prelude::*;
use pob_oracle::schema::StateDump;
use pob_oracle::testkit;
use serde_json::Value as J;

const CS1_LUA: &str = include_str!("../corpus/CS1/cs1.lua");

fn catch_msg<R>(f: impl FnOnce() -> R) -> Result<R, String> {
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(f)).map_err(|p| {
        p.downcast_ref::<LuaError>()
            .map(|e| e.0.clone())
            .or_else(|| p.downcast_ref::<String>().cloned())
            .or_else(|| p.downcast_ref::<&str>().map(|s| (*s).to_owned()))
            .unwrap_or_else(|| "panic".to_owned())
    })
}

fn quiet_lua_errors() {
    let prev = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        if info.payload().downcast_ref::<LuaError>().is_none() {
            prev(info);
        }
    }));
}

// ---- oracle side ------------------------------------------------------------------------------

/// Run PoB on `xml` (patched copy `src`) with the CS1 runtime, writing into `dir`
/// (`state/`, `model/`, `hits.json`, `cs1.json`). Returns the number of forwarded calls.
fn oracle_run(src: &Path, xml: &Path, stem: &str, dir: &Path) -> Result<usize, String> {
    use pob_oracle::headless::Engine;
    let text = std::fs::read_to_string(xml).map_err(|e| e.to_string())?;
    std::fs::create_dir_all(dir.join("state")).map_err(|e| e.to_string())?;
    std::fs::create_dir_all(dir.join("model")).map_err(|e| e.to_string())?;
    let dir = std::fs::canonicalize(dir).map_err(|e| e.to_string())?;
    let root = std::fs::canonicalize(src).map_err(|e| e.to_string())?;
    let mut root_s = root.to_string_lossy().replace('\\', "/");
    if !root_s.ends_with('/') {
        root_s.push('/');
    }
    let opts = pob_oracle::runner::ProbeOptions {
        select: vec!["initEnv:488".to_owned(), "initEnv:800".to_owned()],
        max_dumps: 1_000_000,
        provenance: false,
    };
    let pre = format!(
        "{}\ndo\n{CS1_LUA}\nend\n",
        pob_oracle::runner::probe_prelude(&root_s, &dir, stem, &opts)
    );
    let _lock =
        pob_oracle::runner::OracleLock::acquire().map_err(|e| format!("oracle lock: {e}"))?;
    let mut pob = pob_oracle::jit::JitPob::boot_oracle(&root, Some(&pre))
        .map_err(|e| format!("boot: {e}"))?;
    pob.load_build_xml(&text, stem)
        .map_err(|e| format!("load: {e}"))?;
    pob.run("__cs1.drive(build)")
        .map_err(|e| format!("drive: {e}"))?;
    pob.run("return __probe_finish(__mainObject__.main.modes['BUILD'].calcsTab.mainEnv)")
        .map_err(|e| format!("probe finish: {e}"))?;
    let res = pob
        .run("return __cs1.result()")
        .map_err(|e| format!("result: {e}"))?;
    let json = res.first().map(|r| r.text().to_owned()).unwrap_or_default();
    std::fs::write(dir.join("cs1.json"), &json).map_err(|e| e.to_string())?;
    let j: J = serde_json::from_str(&json).map_err(|e| format!("cs1.json: {e}"))?;
    Ok(j["forwarded"].as_u64().unwrap_or(0) as usize)
}

/// The build's oracle directory (made on first use, reused while complete).
fn ensure_oracle(src: &Path, xml: &Path, stem: &str) -> Result<(PathBuf, usize), String> {
    let dir = Path::new(env!("CARGO_TARGET_TMPDIR"))
        .join("CS1")
        .join(stem);
    let done = dir.join("complete");
    if let Ok(s) = std::fs::read_to_string(&done) {
        if let Ok(n) = s.trim().parse::<usize>() {
            return Ok((dir, n));
        }
    }
    let _ = std::fs::remove_dir_all(&dir);
    let n = oracle_run(src, xml, stem, &dir)?;
    std::fs::write(&done, n.to_string()).map_err(|e| e.to_string())?;
    Ok((dir, n))
}

// ---- the build model --------------------------------------------------------------------------

fn tget(t: &Table, k: &str) -> Val {
    t.get(Name::from(k))
}

fn copy_into(dst: &Table, src: &Val) {
    if let Some(src) = src.table() {
        for (k, v) in src.pairs() {
            dst.set(k, v);
        }
    }
}

fn store(v: &Val, what: &str) -> Result<ModStoreRef, String> {
    v.obj::<ModStore>()
        .ok_or_else(|| format!("model.{what} is not a mod store ({})", v.lua_type()))
}

/// A tree node from a node record (`{id, type, name, isTattoo, overrideType, modList,
/// keystoneMod}`).
fn node_fields(rec: &Table, tree: Option<NodeRef>) -> Result<Node, String> {
    // string ids (e.g. the Kalguur keystones' "kalguur_keystone_1") are kept in `extra`, as CS0's
    // test does; the id is then NaN (never a node-id map key)
    let idv = tget(rec, "id");
    let id = match &idv {
        Val::Num(x) => *x,
        Val::Str(_) => f64::NAN,
        _ => {
            let keys: Vec<String> = rec.pairs().map(|(k, v)| format!("{k:?}={v:?}")).collect();
            return Err(format!("node record without an id: {}", keys.join(" ")));
        }
    };
    let node = Node {
        id,
        tree,
        ..Default::default()
    };
    if let Val::Str(sid) = idv {
        node.extra.set(n!("id"), sid);
    }
    if let Some(t) = tget(rec, "type").str() {
        node.r#type.set(Name::from(&*t));
    }
    if let Some(n) = tget(rec, "name").str() {
        node.extra.set(n!("name"), n);
    }
    node.isTattoo.set(tget(rec, "isTattoo").truthy());
    node.overrideType
        .set(tget(rec, "overrideType").str().map(|s| Name::from(&*s)));
    match tget(rec, "modList") {
        Val::Nil => {}
        v => node.modList.set(store(&v, "node.modList")?),
    }
    match tget(rec, "keystoneMod") {
        Val::Nil => {}
        Val::Mod(m) => node.keystoneMod.set(Some(m)),
        other => return Err(format!("keystoneMod is a {} value", other.lua_type())),
    }
    Ok(node)
}

/// An `LjIntMap` whose `pairs` order is `order`. Lua's table was built by a history the record
/// does not have (allocations, deallocations, rehashes), so this searches for a layout that
/// iterates the same way:
///
/// 1. the keys inserted in `order` into a fresh table;
/// 2. for hash sizes `2^b` from the smallest that holds the keys up to 16 times that (a table
///    that once held more keys keeps its size): a pre-sized table (`table.new(0, 2^b)`) gets
///    first the keys that can sit at their main position in `order` (nodes ascend in `pairs`
///    order, so a key's main position must lie above the previous key's node and leave room
///    for the keys after it), then the others in reverse `order`, so LuaJIT's free pointer,
///    which hands out nodes from the top down, gives the later keys the higher nodes;
/// 3. a bounded repair of (1): move the first out-of-place key one step earlier.
///
/// The second value says whether the found map iterates exactly as `order`.
fn map_in_order(order: &[i64], by_id: &dyn Fn(i64) -> NodeRef) -> (LjIntMap<NodeRef>, bool) {
    let fill = |mut m: LjIntMap<NodeRef>, seq: &[i64]| {
        for k in seq {
            m.set(*k, Some(by_id(*k)));
        }
        m
    };
    let first = fill(LjIntMap::new(), order);
    if first.keys() == order {
        return (first, true);
    }
    let n = order.len().max(1);
    let mut size = n.next_power_of_two();
    while size <= 16 * n.next_power_of_two() {
        let mask = (size - 1) as u32;
        let mut at_mp = Vec::new();
        let mut others = Vec::new();
        let mut taken = std::collections::HashSet::new();
        // `lb`: the lowest node the next key can sit at (nodes ascend in `pairs` order)
        let mut lb: i64 = 0;
        for (i, &k) in order.iter().enumerate() {
            let mp = i64::from(pob_lua::ljtab::hashnum(k) & mask);
            let room = (size as i64 - 1 - mp) >= (n - 1 - i) as i64;
            if mp >= lb && room && taken.insert(mp) {
                at_mp.push(k);
                lb = mp + 1;
            } else {
                others.push(k);
                lb += 1;
            }
        }
        others.reverse();
        let mut seq = at_mp;
        seq.extend(others);
        let m = fill(LjIntMap::with_sizes(0, size as u32), &seq);
        if m.keys() == order {
            return (m, true);
        }
        if std::env::var_os("CS1_ORDER_DEBUG").is_some() && size == n.next_power_of_two() {
            let got = m.keys();
            let i = got.iter().zip(order).position(|(a, b)| a != b).unwrap_or(0);
            eprintln!(
                "S2 size={size} first diff at {i}: lua {:?} ours {:?}",
                &order[i..(i + 6).min(n)],
                &got[i..(i + 6).min(n)]
            );
        }
        size *= 2;
    }
    let mut seq = order.to_vec();
    let mut best = first;
    for _ in 0..(4 * order.len() + 8) {
        let got = best.keys();
        if got == order {
            return (best, true);
        }
        let i = got.iter().zip(order).position(|(a, b)| a != b).unwrap_or(0);
        let want = order[i];
        let p = seq.iter().position(|k| *k == want).unwrap_or(0);
        if p == 0 {
            break;
        }
        seq.swap(p, p - 1);
        best = fill(LjIntMap::new(), &seq);
    }
    let ok = best.keys() == order;
    if !ok && std::env::var_os("CS1_ORDER_DEBUG").is_some() {
        let size = n.next_power_of_two();
        let mask = (size - 1) as u32;
        eprintln!("ORDER n={n} size={size}");
        eprintln!(
            "  lua : {:?}",
            order
                .iter()
                .map(|k| (*k, pob_lua::ljtab::hashnum(*k) & mask))
                .collect::<Vec<_>>()
        );
        eprintln!("  ours: {:?}", fill(LjIntMap::new(), order).keys());
    }
    (best, ok)
}

struct Model {
    build: BuildRef,
    r#override: Rc<Override>,
    order_ok: bool,
}

/// Rebuild the Rust build model and the call's override from `model#k`.
fn model_from(doc: &StateDump) -> Result<Model, String> {
    let inj = pob_oracle::inject::inject(doc, &testkit::empty_build())?;
    let m = inj
        .locals
        .get("model")
        .and_then(Val::table)
        .ok_or("model#k has no model table")?;
    let build = Build::new(data());
    build.characterLevel.set(
        tget(&m, "characterLevel")
            .opt_num()
            .ok_or("characterLevel")?,
    );
    build
        .configTab
        .enemyLevel
        .set(tget(&m, "enemyLevel").opt_num());
    copy_into(&build.configTab.input, &tget(&m, "input"));
    copy_into(&build.configTab.placeholder, &tget(&m, "placeholder"));
    copy_into(&build.calcsTab.input, &tget(&m, "calcsInput"));
    build
        .configTab
        .modList
        .set(store(&tget(&m, "configModList"), "configModList")?);
    build.configTab.enemyModList.set(store(
        &tget(&m, "configEnemyModList"),
        "configEnemyModList",
    )?);
    build
        .partyTab
        .enemyModList
        .set(store(&tget(&m, "partyEnemyModList"), "partyEnemyModList")?);

    // tree: class base stats, keystone map, the allocated nodes' tree nodes
    let classId = tget(&m, "curClassId").opt_num().ok_or("curClassId")?;
    let classes = Table::new();
    if let Some(st) = tget(&m, "classStats").table() {
        classes.set(classId, st);
    }
    let extra = Table::new();
    if let Some(cd) = tget(&m, "characterData").table() {
        let t = Table::new();
        if let Some(st) = tget(&cd, "stats").table() {
            t.set(classId, st);
        }
        extra.set(n!("characterData"), t);
    }
    let keystoneMap = Table::new();
    let mut tree_nodes: LjIntMap<NodeRef> = LjIntMap::new();
    if let Some(ks) = tget(&m, "keystones").table() {
        for (_, rec) in ks.ipairs() {
            let rec = rec.table().ok_or("keystone record")?;
            let node = Rc::new(node_fields(&rec, None)?);
            if !node.id.is_nan() {
                tree_nodes.set(node.id as i64, Some(node.clone()));
            }
            let name = tget(&rec, "name").str().ok_or("keystone name")?;
            keystoneMap.set(Name::from(&*name), Val::Obj(node));
        }
    }
    let alloc_recs: Vec<Table> = tget(&m, "allocNodes")
        .table()
        .map(|t| {
            t.ipairs()
                .into_iter()
                .filter_map(|(_, v)| v.table())
                .collect()
        })
        .unwrap_or_default();
    let ov = tget(&m, "override").table().unwrap_or_default();
    let add_recs: Vec<Table> = tget(&ov, "addNodes")
        .table()
        .map(|t| {
            t.ipairs()
                .into_iter()
                .filter_map(|(_, v)| v.table())
                .collect()
        })
        .unwrap_or_default();
    let mut spec_nodes: LjIntMap<NodeRef> = LjIntMap::new();
    let mut alloc_by_id = std::collections::HashMap::new();
    let mut order = Vec::new();
    let mut spec_node =
        |rec: &Table, tree_nodes: &mut LjIntMap<NodeRef>| -> Result<NodeRef, String> {
            let id = tget(rec, "id").opt_num().ok_or("node id")? as i64;
            if let Some(n) = spec_nodes.get(id) {
                return Ok(n);
            }
            let tn = match tree_nodes.get(id) {
                Some(t) => t,
                None => {
                    let t = Rc::new(node_fields(rec, None)?);
                    tree_nodes.set(id, Some(t.clone()));
                    t
                }
            };
            let n = Rc::new(node_fields(rec, Some(tn))?);
            spec_nodes.set(id, Some(n.clone()));
            Ok(n)
        };
    for rec in &alloc_recs {
        let n = spec_node(rec, &mut tree_nodes)?;
        order.push(n.id as i64);
        alloc_by_id.insert(n.id as i64, n);
    }
    let mut add_nodes = Vec::new();
    for rec in &add_recs {
        add_nodes.push(spec_node(rec, &mut tree_nodes)?);
    }
    let (allocNodes, order_ok) = map_in_order(&order, &|k| alloc_by_id[&k].clone());
    let tree = PassiveTree {
        treeVersion: Name::from(&*tget(&m, "treeVersion").str().unwrap_or_default()),
        nodes: tree_nodes,
        classes,
        keystoneMap,
        extra,
        ..Default::default()
    };
    let spec = PassiveSpec::default();
    spec.tree.set(Rc::new(tree));
    spec.treeVersion.set(Name::from(
        &*tget(&m, "treeVersion").str().unwrap_or_default(),
    ));
    spec.nodes.with_mut(|n| *n = spec_nodes);
    spec.allocNodes.with_mut(|n| *n = allocNodes);
    spec.curClassId.set(classId);
    spec.allocatedNotableCount
        .set(tget(&m, "allocatedNotableCount").num());
    spec.allocatedKeystoneCount
        .set(tget(&m, "allocatedKeystoneCount").num());
    spec.allocatedMasteryCount
        .set(tget(&m, "allocatedMasteryCount").num());
    spec.allocatedMasteryTypeCount
        .set(tget(&m, "allocatedMasteryTypeCount").num());
    copy_into(
        &spec.allocatedMasteryTypes,
        &tget(&m, "allocatedMasteryTypes"),
    );
    copy_into(
        &spec.allocatedTattooTypes,
        &tget(&m, "allocatedTattooTypes"),
    );
    build.spec.set(Rc::new(spec));

    // the call's override
    if tget(&ov, "spec").truthy() {
        return Err("override.spec is set (not driven by this test)".to_owned());
    }
    if let Some(other) = tget(&ov, "other").str() {
        return Err(format!("override has other keys: {other}"));
    }
    let o = Override::default();
    if tget(&ov, "addNodes").truthy() {
        let mut a = LjIntMap::new();
        for n in &add_nodes {
            a.set(n.id as i64, Some(n.clone()));
        }
        o.addNodes.set(Some(Shared::new(a)));
    }
    if let Some(rem) = tget(&ov, "removeNodes").table() {
        let mut r = LjIntMap::new();
        for (_, id) in rem.ipairs() {
            let id = id.opt_num().ok_or("removeNodes id")? as i64;
            let n = alloc_by_id
                .get(&id)
                .cloned()
                .ok_or_else(|| format!("removeNodes: {id} is not allocated"))?;
            r.set(id, Some(n));
        }
        o.removeNodes.set(Some(Shared::new(r)));
    }
    if let Some(c) = tget(&ov, "conditions").table() {
        let list: Vec<Name> = c
            .ipairs()
            .into_iter()
            .filter_map(|(_, v)| v.str().map(|s| Name::from(&*s)))
            .collect();
        o.conditions.set(Some(Shared::new(list)));
    }
    match tget(&ov, "extraJewelFuncs") {
        Val::Nil => {}
        v => o
            .extraJewelFuncs
            .set(Some(store(&v, "override.extraJewelFuncs")?)),
    }
    Ok(Model {
        build,
        r#override: Rc::new(o),
        order_ok,
    })
}

// ---- L4 -----------------------------------------------------------------------------------------

/// One pair: inject `initEnv:488#k` onto the model, run CS1, diff against `initEnv:800#k`.
fn check_pair(dir: &Path, k: usize, stem: &str) -> Result<(Vec<String>, bool), String> {
    let model_doc = StateDump::load(&dir.join(format!("model/model#{k}.json")))
        .map_err(|e| format!("model#{k}: {e}"))?;
    let model = model_from(&model_doc)?;
    let state = dir.join("state");
    let din = StateDump::load(&state.join(format!("initEnv:488#{k}.json")))
        .map_err(|e| format!("initEnv:488#{k}: {e}"))?;
    let dout = StateDump::load(&state.join(format!("initEnv:800#{k}.json")))
        .map_err(|e| format!("initEnv:800#{k}: {e}"))?;
    let inj = pob_oracle::inject::inject(&din, &model.build)?;
    let mut locals: Vec<(String, Val)> = inj.locals.into_iter().collect();
    // The override local: a fresh `{ … }` table (decoded as a plain table) is replaced by the
    // model's override; an `env.override` reference (the env paths) is the injected Override
    // itself. The replaced values stay alive until the export: the probe registry keys injected
    // tables by address, so a freed injected table whose address a new table reuses would be
    // exported with the injected table's contents.
    let mut keep_alive: Vec<Val> = Vec::new();
    for (name, v) in &mut locals {
        if name == "override" && v.obj::<Override>().is_none() {
            // `override.extraJewelFuncs` (CS2's, passed through): the dumped store itself, whose
            // actor is another env's player (`$foreign`)
            if let Some(ejf) = v.table().map(|t| t.get(n!("extraJewelFuncs"))) {
                if let Some(st) = ejf.obj::<ModStore>() {
                    model.r#override.extraJewelFuncs.set(Some(st));
                }
            }
            keep_alive.push(std::mem::replace(v, Val::Obj(model.r#override.clone())));
        }
    }
    // The injected state exported before the run, against the `:in` dump: a mismatch that is
    // identical before and after the run (same path, same values on both sides) is a property
    // of the injection round trip on state the segment does not touch (e.g. a GlobalCache
    // entry's placeholder skill), not of the segment.
    let before = { testkit::diff_against(&din, inj.env.as_ref(), &locals, &format!("CS1-{stem}")) };
    let before: std::collections::HashSet<String> = before.iter().map(mismatch_key).collect();
    let run = catch_msg(|| pob_calc::probe::run_segment(&din.probe, &locals));
    let after = match run {
        Ok(Ok((_flow, after))) => after,
        Ok(Err(e)) => return Ok((vec![format!("run: {e}")], model.order_ok)),
        Err(e) => return Ok((vec![format!("run raised: {e}")], model.order_ok)),
    };
    let env = after
        .iter()
        .find(|(n, _)| n == "env")
        .and_then(|(_, v)| v.obj::<Env>())
        .or(inj.env);
    if std::env::var_os("CS1_DUMP_OURS").is_some() {
        let ours = testkit::export_after(env.as_ref(), &after);
        let _ = std::fs::write(
            dir.join(format!("ours#{k}.json")),
            serde_json::to_string_pretty(&ours).unwrap_or_default(),
        );
    }
    // no nondeterminism record: both dumps come from one PoB process
    let mm = testkit::diff_against(&dout, env.as_ref(), &after, &format!("CS1-{stem}"));
    drop(keep_alive);
    let mut kept = Vec::new();
    for m in mm {
        let why = if eager_struct_field(&m) {
            "eager struct field"
        } else if before.contains(&mismatch_key(&m)) {
            "unchanged injection round-trip difference"
        } else if !model.order_ok
            && m.path == "env.initialNodeModDB.mods"
            && format!("{:?}", m.kind) == "LuaOrder"
        {
            // same multiset of mods; the order follows `spec.allocNodes`' LuaJIT layout, which
            // the rebuilt spec could not reproduce (counted separately below)
            ORDER_LIMITED.with(|c| c.set(c.get() + 1));
            continue;
        } else if accelerate_ref(&m) {
            "specEnv.accelerate identity"
        } else if untouched_subtree(&m) {
            "state CS1 neither reads nor writes (GlobalCache, override.extraJewelFuncs)"
        } else {
            kept.push(m);
            continue;
        };
        FILTERED.with(|f| *f.borrow_mut().entry(why).or_default() += 1);
    }
    let n = kept.len();
    let mm = node_sets(kept);
    FILTERED.with(|f| {
        *f.borrow_mut()
            .entry("override node set (same ids)")
            .or_default() += n - mm.len();
    });
    Ok((
        mm.into_iter()
            .map(|m| format!("{} {:?}: pob={:?} ours={:?}", m.path, m.kind, m.pob, m.ours))
            .collect(),
        model.order_ok,
    ))
}

/// The Rust `Env` / `Actor` / `SpecEnv` structs have every field from their construction
/// (`Env::new`, `Actor::default`, `SpecEnv`'s `accelerate`), where PoB's tables only get these
/// keys later: the env's `buffs`, `minionBuffs`, `debuffs`, `curseSlots`, `requirementsTable`,
/// `flaskSlotMap`, `flaskSlotOccupied`, `crossLinkedSupportGroups` (CalcSetup.lua 800+,
/// CalcPerform), the actors' `output`, `weaponData1/2`, `strDmgBonus`, `hostile` (CalcSetup.lua
/// 800+, CalcPerform) and the enemy's `itemList` / `activeSkillList`, and `specEnv.accelerate`
/// (Calcs.lua passes a specEnv without it). Such a field is exported with its default value
/// although PoB's table has no key yet: an `Extra` mismatch with PoB's side absent and ours the
/// field's default is that representation difference of the frozen structs (calc doc §5.4), not
/// a CS1 result.
fn eager_struct_field(m: &pob_oracle::diff::Mismatch) -> bool {
    const ENV: &[&str] = &[
        "env.buffs",
        "env.minionBuffs",
        "env.debuffs",
        "env.curseSlots",
        "env.requirementsTable",
        "env.flaskSlotMap",
        "env.flaskSlotOccupied",
        "env.crossLinkedSupportGroups",
        "locals.specEnv.accelerate",
    ];
    const ACTOR: &[&str] = &[
        "output",
        "weaponData1",
        "weaponData2",
        "strDmgBonus",
        "hostile",
        "itemList",
        "activeSkillList",
    ];
    let field = ENV.contains(&m.path.as_str())
        || m.path
            .strip_prefix("env.player.")
            .or_else(|| m.path.strip_prefix("env.enemy."))
            .is_some_and(|rest| ACTOR.contains(&rest));
    let default = match m.ours.as_ref() {
        Some(J::Object(o)) => o.is_empty(),
        Some(J::Bool(b)) => !b,
        Some(J::Number(n)) => n.as_f64() == Some(0.0),
        _ => false,
    };
    field && m.pob.is_none() && default && format!("{:?}", m.kind) == "Extra"
}

/// State the segment neither reads nor writes, so a difference there comes from the injection
/// / export round trip (or the rebuilt model), never from CS1: the GlobalCache (CalcSetup.lua
/// 488-799 and the Rust segment never touch it; its entries' skills and envs belong to other
/// envs, and their dump metadata, e.g. `"$uuid"` = `cacheSkillUUID(skill, <the dump's env>)`,
/// depends on the env the dump is rooted at) and `override.extraJewelFuncs` (set by CS2 at
/// CalcSetup.lua:1424-1430 before a restart and only passed through here; its store's actor is
/// another env's player).
fn untouched_subtree(m: &pob_oracle::diff::Mismatch) -> bool {
    m.path.starts_with("globalCache.")
        || m.path.starts_with("env.override.extraJewelFuncs")
        || m.path.starts_with("locals.override.extraJewelFuncs")
}

thread_local! {
    /// `env.initialNodeModDB` compared as a multiset (order not reproducible; see `map_in_order`).
    static ORDER_LIMITED: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
    /// Representation differences dropped by the filters below, by reason.
    static FILTERED: std::cell::RefCell<std::collections::BTreeMap<&'static str, usize>> =
        std::cell::RefCell::new(Default::default());
}

fn mismatch_key(m: &pob_oracle::diff::Mismatch) -> String {
    format!("{} {:?} {:?} {:?}", m.path, m.kind, m.pob, m.ours)
}

/// `specEnv.accelerate` is a `Copy` struct (`SpecEnv.accelerate: Accelerate`), so the table
/// identity PoB's dump records (`specEnv.accelerate` is the `accelerate` local, CalcSetup.lua:482)
/// cannot be exported; its contents are compared at `locals.accelerate`.
fn accelerate_ref(m: &pob_oracle::diff::Mismatch) -> bool {
    m.path == "locals.specEnv.accelerate"
        && format!("{:?}", m.kind) == "Value"
        && m.pob
            .as_ref()
            .and_then(|p| p.get("$ref"))
            .and_then(J::as_str)
            == Some("locals.accelerate")
}

/// `override.addNodes` / `override.removeNodes` are node-keyed sets in PoB (`{ [node] = true }`)
/// and node-id maps of the node in the port (`Override.addNodes: LjIntMap<NodeRef>`, calc doc
/// §5.4). Under each of the two paths the mismatches PoB's `[@i:key] = $node X` / `[@i] = true`
/// against ours `[X] = $node X` are dropped when both sides hold the same node ids; any other
/// mismatch under those paths is kept.
fn node_sets(mm: Vec<pob_oracle::diff::Mismatch>) -> Vec<pob_oracle::diff::Mismatch> {
    let node_id = |j: &Option<J>| j.as_ref().and_then(|v| v.get("$node")).and_then(J::as_f64);
    let mut out = Vec::new();
    let mut groups: std::collections::BTreeMap<String, Vec<pob_oracle::diff::Mismatch>> =
        Default::default();
    for m in mm {
        let base = ["env.override.addNodes", "env.override.removeNodes"]
            .into_iter()
            .find(|b| m.path.starts_with(&format!("{b}[")));
        match base {
            Some(b) => groups.entry(b.to_owned()).or_default().push(m),
            None => out.push(m),
        }
    }
    for (base, g) in groups {
        let mut pob_ids = Vec::new();
        let mut our_ids = Vec::new();
        let mut other = false;
        for m in &g {
            let rest = &m.path[base.len()..];
            let kind = format!("{:?}", m.kind);
            if rest.starts_with("[@") && rest.ends_with(":key]") && kind == "Missing" {
                match node_id(&m.pob) {
                    Some(x) => pob_ids.push(x),
                    None => other = true,
                }
            } else if rest.starts_with("[@") && kind == "Missing" && m.pob == Some(J::Bool(true)) {
            } else if kind == "Extra" {
                match node_id(&m.ours) {
                    Some(x) if rest == format!("[{x}]") => our_ids.push(x),
                    _ => other = true,
                }
            } else {
                other = true;
            }
        }
        pob_ids.sort_by(f64::total_cmp);
        our_ids.sort_by(f64::total_cmp);
        if other || pob_ids != our_ids {
            out.extend(g);
        }
    }
    out
}

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
    if let Ok(list) = std::env::var("CS1_BUILDS") {
        let want: Vec<&str> = list.split(',').map(str::trim).collect();
        v.retain(|p| {
            p.file_stem()
                .is_some_and(|s| want.contains(&s.to_string_lossy().as_ref()))
        });
    }
    if let Some(n) = std::env::var("CS1_MAX_BUILDS")
        .ok()
        .and_then(|s| s.parse::<usize>().ok())
    {
        v.truncate(n);
    }
    v
}

#[test]
fn cs1_l4_initEnv_488_to_800() {
    let Some(_dump) = pob_testpaths::require(pob_testpaths::dump_dir(), "data/dump") else {
        return;
    };
    assert!(testkit::init_default_data(), "data/dump does not load");
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
    let verbose = std::env::var_os("CS1_VERBOSE").is_some();
    let stages = Path::new(env!("CARGO_MANIFEST_DIR")).join("corpus/CS1/stages.toml");
    let root = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("oracle-src");
    let patched = pob_oracle::patcher::patch(&pob_src, Some(&stages), &root)
        .unwrap_or_else(|e| panic!("patch: {e:?}"));
    let (mut pairs, mut ok, mut bad_order, mut bad_order_failed) = (0, 0, 0, 0);
    let mut failures: Vec<String> = Vec::new();
    let mut oracle_failures = Vec::new();
    let mut shapes: std::collections::BTreeMap<String, (usize, usize)> = Default::default();
    let t0 = std::time::Instant::now();
    for xml in &corpus {
        let stem = xml.file_stem().unwrap().to_string_lossy().into_owned();
        let (dir, n) = match ensure_oracle(&patched.src, xml, &stem) {
            Ok(x) => x,
            Err(e) => {
                oracle_failures.push(format!("{stem}: {e}"));
                continue;
            }
        };
        let info: J = std::fs::read_to_string(dir.join("cs1.json"))
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or(J::Null);
        let mut build_ok = 0;
        for k in 1..=n {
            let shape = info["shapes"][k - 1].as_str().unwrap_or("?").to_owned();
            pairs += 1;
            let e = shapes.entry(shape.clone()).or_default();
            e.0 += 1;
            match check_pair(&dir, k, &stem) {
                Ok((mm, order_ok)) => {
                    if !order_ok {
                        bad_order += 1;
                    }
                    if mm.is_empty() {
                        ok += 1;
                        build_ok += 1;
                        e.1 += 1;
                    } else {
                        if !order_ok {
                            bad_order_failed += 1;
                        }
                        failures.push(format!(
                            "{stem} #{k} ({shape}{}): {} mismatches",
                            if order_ok {
                                ""
                            } else {
                                ", order not reproduced"
                            },
                            mm.len()
                        ));
                        for m in mm.iter().take(if verbose { usize::MAX } else { 12 }) {
                            failures.push(format!("      {m}"));
                        }
                    }
                }
                Err(e) => failures.push(format!("{stem} #{k} ({shape}): {e}")),
            }
        }
        if verbose {
            eprintln!(
                "{stem}: {build_ok}/{n} pairs identical ({:.0}s)",
                t0.elapsed().as_secs_f64()
            );
        }
    }
    eprintln!(
        "CS1 L4 initEnv:488 -> initEnv:800: {ok}/{pairs} pairs identical over {} builds ({} oracle failures) in {:.0}s",
        corpus.len(),
        oracle_failures.len(),
        t0.elapsed().as_secs_f64()
    );
    eprintln!("  by shape (identical/pairs):");
    for (s, (n, k)) in &shapes {
        eprintln!("    {s}: {k}/{n}");
    }
    FILTERED.with(|f| {
        eprintln!(
            "  representation differences dropped (not CS1 results; see the filters): {:?}",
            f.borrow()
        );
    });
    eprintln!(
        "  allocNodes order not reproduced by the rebuilt spec: {bad_order} pairs ({bad_order_failed} of them failed); in {} of them env.initialNodeModDB was compared as a multiset (same mods, other order)",
        ORDER_LIMITED.with(std::cell::Cell::get)
    );
    for f in &oracle_failures {
        eprintln!("  ORACLE FAILURE {f}");
    }
    for f in failures.iter().take(if verbose { usize::MAX } else { 200 }) {
        eprintln!("  {f}");
    }
    assert!(oracle_failures.is_empty(), "oracle failures");
    assert!(pairs > 0, "no pairs");
    assert_eq!(ok, pairs, "CS1 L4 mismatches");
}
