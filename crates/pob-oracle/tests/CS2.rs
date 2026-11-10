//! Unit CS2 acceptance (calc doc §11.1 row CS2, §9.4 L4): initEnv segment CS2
//! (Modules/CalcSetup.lua 800-1450, `pob_calc::modules::calc_setup::init_env_items::seg`),
//! L4 `initEnv:800#k` → `initEnv:1451#k`, including the Restart path (1433, extraJewelFuncs)
//! and the `toggleFlask` / `repItem` overrides.
//!
//! **Oracle side.** PoB is patched with `corpus/CS2/stages.toml` (the `initEnv:800` /
//! `initEnv:1451` probes of docs/port/stages.toml) and one `pob_oracle probe` run per build dumps
//! every hit of both probes in one process (so `:in` and `:out` pair up), into
//! `$CARGO_TARGET_TMPDIR/CS2/<hash of the stages>/dumps/<stem>/` (reused while complete). A hit's
//! call is the innermost `initEnv:in#j` of its probe stack: the 800 and 1451 hits of one call pair
//! up; a call with an 800 hit and no 1451 hit is PoB's restart at 1433 (its recursive call is the
//! next `initEnv:in`).
//!
//! **Rust side (ORC-X testkit).** The build model is BM-A's
//! (`build_xml::loadBuildModelFromXML`, the model `Build:Init` leaves before its first
//! BuildOutput), loaded once per build; `initEnv:800#k` is injected onto it, the Rust segment runs
//! on the injected locals (`probe::run_segment`), and `testkit::diff_against` compares env and
//! locals with `initEnv:1451#k` (strict; both dumps come from one process). A restart hit must
//! return `Flow::Restart` with `override.extraJewelFuncs` holding as many mods as PoB's recursive
//! call received. Hits PoB made before its build had loaded (its `env.player.itemList` at 1451 is
//! empty while the model has equipped items: `Build:Init`'s calls before the item set loads)
//! are skipped and counted (`pre-load`).
//!
//! **Driven overrides.** After the build load the oracle side runs no extra calls; the
//! calculator envs PoB makes during the load (`getMiscCalculator`, power) carry `override.repItem`
//! / `toggleFlask` only when the build exercises them. `cs2_l4_overrides` therefore runs the Rust
//! segment with a `repSlotName`/`repItem` override and with a `toggleFlask` override on each
//! build's main env hit and checks the effect against the Lua semantics (item swapped in, flask
//! toggled) directly.
//!
//! **Representation differences** (counted and printed, not gated):
//! * `round-trip`: a mismatch identical before and after the run (the injected state exported
//!   before running already differs in the same way), i.e. a property of inject/export, not CS2;
//! * `explodeSources (CCR-0030 #3)`: PoB's list holds items and nodes; the port keeps the Lua list
//!   in `env.extra["explodeSources"]` (the typed field exports empty). The extra list is compared
//!   against PoB's instead (same length and the same item / node identities in order);
//! * `rust-only plumbing`: `env.$flaskSlotMap` (CCR-0030 #5 side map) and
//!   `override.$extraJewelFuncsActor` (keeps the actor alive);
//! * `eager struct field`: an `Extra` mismatch of a typed field PoB has not set yet whose Rust
//!   value is the default (calc doc §5.4 structs; CCR-0082 §3);
//! * `globalCache`, `override.extraJewelFuncs` of a second pass (the dumped store's actor is
//!   another env's player, `$foreign`).
//!
//! Gate: no hit raises, every restart hit restarts, and the share of compared hits with zero
//! unexplained mismatches is at least `CS2_MIN_PASS_PCT` (default 98, calc doc §9.4 L4 all57).
//!
//! Inputs: `data/dump`, the PoB checkout, `.parity/builds`. Knobs: `CS2_BUILDS=stem1,stem2`,
//! `CS2_VERBOSE=1`, `CS2_MAX_HITS=n` (per build), `CS2_MIN_PASS_PCT`.
#![allow(non_snake_case, clippy::too_many_lines)]

use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::{Arc, OnceLock};

use pob_calc::prelude::*;
use pob_gamedata::timeless_lut as lut;
use pob_oracle::diff::Mismatch;
use pob_oracle::schema::StateDump;
use pob_oracle::testkit;
use serde_json::Value as J;

fn serial() -> std::sync::MutexGuard<'static, ()> {
    static M: std::sync::Mutex<()> = std::sync::Mutex::new(());
    M.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
}

fn corpus_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("corpus/CS2")
}

fn tmp_dir() -> PathBuf {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    std::fs::read_to_string(corpus_dir().join("stages.toml"))
        .unwrap_or_default()
        .hash(&mut h);
    Path::new(env!("CARGO_TARGET_TMPDIR"))
        .join("CS2")
        .join(format!("{:016x}", h.finish()))
}

fn stem(p: &Path) -> String {
    p.file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or_default()
        .to_owned()
}

fn quiet_lua_errors() {
    static ONCE: std::sync::Once = std::sync::Once::new();
    ONCE.call_once(|| {
        let prev = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |info| {
            if info.payload().downcast_ref::<LuaError>().is_none() {
                prev(info);
            }
        }));
    });
}

fn catch<R>(f: impl FnOnce() -> R) -> Result<R, String> {
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(f)).map_err(|p| {
        p.downcast_ref::<LuaError>()
            .map(|e| e.0.clone())
            .or_else(|| p.downcast_ref::<String>().cloned())
            .or_else(|| p.downcast_ref::<&str>().map(|s| (*s).to_owned()))
            .unwrap_or_else(|| "panic".to_owned())
    })
}

/// Timeless jewel LUTs load from the PoB checkout (as BM-A / CA do).
fn setup_luts(src: &Path) {
    let src = std::fs::canonicalize(src).unwrap_or_else(|_| src.to_path_buf());
    lut::set_jewel_fs(Arc::new(lut::StdJewelFs {
        script_path: src.to_string_lossy().into_owned(),
        writable: false,
    }));
    lut::set_con_printf(|_| {});
}

/// all57 (`.parity/builds/*.xml`), filtered by `CS2_BUILDS`.
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
    if let Ok(list) = std::env::var("CS2_BUILDS") {
        let want: Vec<&str> = list.split(',').map(str::trim).collect();
        v.retain(|p| want.contains(&stem(p).as_str()));
    }
    v
}

// ---- oracle side --------------------------------------------------------------------------------

fn patched_src(pob_src: &Path) -> Result<PathBuf, String> {
    static P: OnceLock<Result<PathBuf, String>> = OnceLock::new();
    P.get_or_init(|| {
        let root = tmp_dir().join("patched");
        std::fs::create_dir_all(&root).map_err(|e| e.to_string())?;
        let out = std::process::Command::new(env!("CARGO_BIN_EXE_pob_oracle"))
            .arg("patch")
            .arg("--stages")
            .arg(corpus_dir().join("stages.toml"))
            .arg("--pob")
            .arg(pob_src)
            .arg("--root")
            .arg(&root)
            .stderr(std::process::Stdio::null())
            .output()
            .map_err(|e| format!("pob_oracle patch: {e}"))?;
        if !out.status.success() {
            return Err(format!("pob_oracle patch failed ({})", out.status));
        }
        Ok(PathBuf::from(
            String::from_utf8_lossy(&out.stdout)
                .lines()
                .last()
                .unwrap_or_default()
                .trim(),
        ))
    })
    .clone()
}

fn ensure_dumps(pob_src: &Path, xml: &Path, stem: &str) -> Result<PathBuf, String> {
    let dir = tmp_dir().join("dumps").join(stem);
    let done = dir.join("complete");
    if done.is_file() {
        return Ok(dir);
    }
    let src = patched_src(pob_src)?;
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    let status = std::process::Command::new(env!("CARGO_BIN_EXE_pob_oracle"))
        .arg("probe")
        .arg(xml)
        .arg("--out-dir")
        .arg(&dir)
        .arg("--patched")
        .arg(&src)
        .arg("--select")
        .arg("initEnv:800,initEnv:1451")
        .arg("--max-dumps")
        .arg("400")
        .arg("--no-provenance")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map_err(|e| format!("pob_oracle probe: {e}"))?;
    if !status.success() || !dir.join("state").is_dir() {
        return Err(format!("pob_oracle probe {} failed ({status})", xml.display()));
    }
    std::fs::write(&done, b"").map_err(|e| e.to_string())?;
    Ok(dir)
}

/// Dump files of a probe by hit number.
fn hit_files(state: &Path, probe: &str) -> BTreeMap<u64, PathBuf> {
    let mut out = BTreeMap::new();
    for e in std::fs::read_dir(state).into_iter().flatten().filter_map(Result::ok) {
        let name = e.file_name().to_string_lossy().into_owned();
        let base = name
            .strip_suffix(".json.zst")
            .or_else(|| name.strip_suffix(".json"))
            .unwrap_or("");
        if let Some((p, n)) = base.split_once('#') {
            if p == probe {
                if let Ok(n) = n.parse::<u64>() {
                    out.insert(n, e.path());
                }
            }
        }
    }
    out
}

/// The innermost `initEnv:in#j` of a dump's probe stack (the initEnv call the hit belongs to).
fn call_of(path: &Path) -> Option<String> {
    let text = std::fs::read_to_string(path).ok()?;
    let j: J = serde_json::from_str(&text).ok()?;
    j.get("stack")?
        .as_array()?
        .iter()
        .rev()
        .filter_map(J::as_str)
        .find(|s| s.starts_with("initEnv:in#"))
        .map(str::to_owned)
}

// ---- the run ------------------------------------------------------------------------------------

/// The `override` local as an `Override` (a fresh `{ … }` table at 485 decodes as a plain table:
/// copy its keys into one; `env.override` paths are already the injected object).
fn as_override(v: &Val) -> Option<Rc<Override>> {
    if let Some(o) = v.obj::<Override>() {
        return Some(o);
    }
    let t = v.table()?;
    let o = Override::default();
    for (k, x) in t.pairs() {
        match k.name().map(|n| n.as_str().to_owned()).as_deref() {
            Some("repSlotName") => o.repSlotName.set(x.str().map(|s| Name::from(&*s))),
            Some("repItem") => o.repItem.set(x.obj::<Item>()),
            Some("toggleFlask") => o.toggleFlask.set(x.obj::<Item>()),
            Some("toggleTincture") => o.toggleTincture.set(x.obj::<Item>()),
            Some("extraJewelFuncs") => o.extraJewelFuncs.set(x.obj::<ModStore>()),
            Some("spec") => o.spec.set(x.obj::<PassiveSpec>()),
            _ => o.extra.set(k, x),
        }
    }
    Some(Rc::new(o))
}

enum Outcome {
    Pass(BTreeMap<&'static str, usize>),
    Fail(Vec<String>, BTreeMap<&'static str, usize>),
    Restart,
    RestartWrong(String),
    PreLoad,
    Error(String),
}

fn key(m: &Mismatch) -> String {
    format!("{} {:?} {:?} {:?}", m.path, m.kind, m.pob, m.ours)
}

fn kind(m: &Mismatch) -> String {
    format!("{:?}", m.kind)
}

/// `Extra` mismatches of typed struct fields PoB has not set yet, with a default Rust value.
fn eager_field(m: &Mismatch) -> bool {
    let default = match m.ours.as_ref() {
        Some(J::Object(o)) => o.is_empty() || o.get("$seq").is_some_and(|s| s.as_array().is_some_and(Vec::is_empty)),
        Some(J::Array(a)) => a.is_empty(),
        Some(J::Bool(b)) => !b,
        Some(J::Number(n)) => n.as_f64() == Some(0.0),
        _ => false,
    };
    kind(m) == "Extra" && m.pob.is_none() && default
}

/// The ids of the items / nodes of a dumped list (`{"$ref": id}` / `{"$node": id}` / `$id`), in
/// order, resolved against the document's `$id` index.
fn list_ids(root: &J, v: Option<&J>) -> Vec<String> {
    let Some(v) = v else { return Vec::new() };
    let arr: Vec<J> = match v {
        J::Array(a) => a.clone(),
        J::Object(o) => o
            .get("$seq")
            .and_then(J::as_array)
            .cloned()
            .unwrap_or_default(),
        _ => Vec::new(),
    };
    arr.iter()
        .map(|e| {
            let e = resolve(root, e);
            if let Some(n) = e.get("$item").and_then(J::as_str) {
                format!("item {n}")
            } else if let Some(n) = e.get("$node") {
                format!("node {n}")
            } else if let Some(n) = e.get("$treeNode") {
                format!("node {n}")
            } else if let Some(name) = e.get("name").and_then(J::as_str) {
                format!("item {name}")
            } else {
                format!("other {e}")
            }
        })
        .collect()
}

/// Resolve a diff path (`env.modDB.mods.Str`, `env.player.itemList["Ring 1"]`, `x[3]`) in a
/// dumped / exported tree (`$num` / `$seq` containers included).
fn at_path<'a>(root: &'a J, path: &str) -> Option<&'a J> {
    let mut cur = root;
    let mut rest = path;
    while !rest.is_empty() {
        let (seg, next): (String, &str) = if let Some(r) = rest.strip_prefix('.') {
            let end = r.find(['.', '[']).unwrap_or(r.len());
            (r[..end].to_owned(), &r[end..])
        } else if let Some(r) = rest.strip_prefix("[\"") {
            let end = r.find("\"]")?;
            (r[..end].to_owned(), &r[end + 2..])
        } else if let Some(r) = rest.strip_prefix('[') {
            let end = r.find(']')?;
            (r[..end].to_owned(), &r[end + 1..])
        } else {
            let end = rest.find(['.', '[']).unwrap_or(rest.len());
            (rest[..end].to_owned(), &rest[end..])
        };
        rest = next;
        // `[@i:key]` / `[@i]`: the i-th entry of an object-keyed table (`$keys` pairs)
        if let Some(at) = seg.strip_prefix('@') {
            let (i, is_key) = match at.strip_suffix(":key") {
                Some(i) => (i, true),
                None => (at, false),
            };
            let pair = cur.get("$keys")?.as_array()?.get(i.parse::<usize>().ok()?.checked_sub(1)?)?;
            cur = pair.get(usize::from(!is_key))?;
            continue;
        }
        cur = if let Some(v) = cur.get(&seg) {
            v
        } else if let Some(v) = cur.get("$num").and_then(|n| n.get(&seg)) {
            v
        } else if let (Some(a), Ok(i)) = (cur.get("$seq").and_then(J::as_array).or_else(|| cur.as_array()), seg.parse::<usize>()) {
            a.get(i.checked_sub(1)?)?
        } else {
            return None;
        };
    }
    Some(cur)
}

/// The mods of a store's name list that do not come from tree nodes, in order: a `LUA-ORDER`
/// difference among tree-node mods only follows `env.allocNodes`' LuaJIT layout, which injection
/// rebuilds from the dump without PoB's insertion history.
fn order_outside_tree_equal(pob: &J, ours: &J, path: &str) -> bool {
    let list = |t: &J| -> Option<Vec<String>> {
        let v = at_path(t, path)?;
        let a = v.as_array().cloned().or_else(|| v.get("$seq").and_then(J::as_array).cloned())?;
        Some(
            a.iter()
                .filter(|m| !m.get("source").and_then(J::as_str).is_some_and(|s| s.starts_with("Tree")))
                .map(|m| {
                    let mut m = m.clone();
                    if let Some(o) = m.as_object_mut() {
                        o.remove("$at");
                        o.remove("$id");
                    }
                    m.to_string()
                })
                .collect(),
        )
    };
    match (list(pob), list(ours)) {
        (Some(a), Some(b)) => a == b,
        _ => false,
    }
}

/// The object a `{"$ref": <path or id>}` points at (else the value itself).
fn resolve<'a>(root: &'a J, v: &'a J) -> &'a J {
    fn by_id<'a>(t: &'a J, id: u64) -> Option<&'a J> {
        match t {
            J::Object(o) => {
                if o.get("$id").and_then(J::as_u64) == Some(id) {
                    return Some(t);
                }
                o.values().find_map(|x| by_id(x, id))
            }
            J::Array(a) => a.iter().find_map(|x| by_id(x, id)),
            _ => None,
        }
    }
    match v.get("$ref") {
        Some(J::String(path)) => at_path(root, path).unwrap_or(v),
        Some(J::Number(n)) => n.as_u64().and_then(|id| by_id(root, id)).unwrap_or(v),
        _ => v,
    }
}

/// The identity of an item / node value (`$item` name, `$node` id), after `$ref`s.
fn ident(root: &J, v: &J) -> Option<String> {
    let v = resolve(root, v);
    if let Some(n) = v.get("$item").and_then(J::as_str) {
        return Some(format!("item {n}"));
    }
    if let Some(n) = v.get("$node").or_else(|| v.get("$treeNode")) {
        return Some(format!("node {n}"));
    }
    None
}

/// Strip the representation of jewel funcs: PoB's closure `{"$fn": src}` and the port's
/// `Value::JewelFunc` (exported as `{ func = {"$fn": key}, type = … }`, a value table wrapping it
/// once more) become `"fn"`; `$at` / `$id` are dropped.
fn norm_fn(v: &J) -> J {
    match v {
        J::Object(o) => {
            if o.contains_key("$fn") {
                return J::from("fn");
            }
            if let (Some(f), Some(_)) = (o.get("func"), o.get("type")) {
                if f.get("func").is_some() && f.get("type").is_some() {
                    return norm_fn(f);
                }
            }
            J::Object(
                o.iter()
                    .filter(|(k, _)| *k != "$at" && *k != "$id")
                    .map(|(k, x)| (k.clone(), norm_fn(x)))
                    .collect(),
            )
        }
        J::Array(a) => J::Array(a.iter().map(norm_fn).collect()),
        x => x.clone(),
    }
}

/// The mod list at `path` on both sides is the same multiset once jewel funcs are normalised.
fn mods_equal_norm(pob: &J, ours: &J, path: &str) -> bool {
    let list = |t: &J| -> Option<Vec<String>> {
        let v = at_path(t, path)?;
        let a = v.as_array().cloned().or_else(|| v.get("$seq").and_then(J::as_array).cloned())?;
        let mut l: Vec<String> = a.iter().map(|m| norm_fn(m).to_string()).collect();
        l.sort();
        Some(l)
    };
    matches!((list(pob), list(ours)), (Some(a), Some(b)) if a == b)
}

/// Every `env.radiusJewelList[i].nodes` holds the same node ids as PoB's.
fn radius_nodes_equal(env: &EnvRef, dout: &StateDump) -> bool {
    let pob = dout.root.get("env").and_then(|e| e.get("radiusJewelList"));
    let pob: Vec<J> = pob
        .and_then(|v| v.as_array().cloned().or_else(|| v.get("$seq").and_then(J::as_array).cloned()))
        .unwrap_or_default();
    let ours = env.radiusJewelList.get().snapshot();
    if pob.len() != ours.len() {
        return false;
    }
    for (p, o) in pob.iter().zip(&ours) {
        let mut pids: Vec<i64> = p
            .get("nodes")
            .map(|n| resolve(&dout.root, n))
            .and_then(|n| n.get("$num").or(Some(n)))
            .and_then(J::as_object)
            .map(|m| m.keys().filter_map(|k| k.parse().ok()).collect())
            .unwrap_or_default();
        let mut oids: Vec<i64> = match o.get(n!("nodes")) {
            Val::Obj(x) => x.downcast_ref::<LjIntMap<NodeRef>>().map(LjIntMap::keys).unwrap_or_default(),
            Val::Table(t) => t
                .pairs()
                .filter_map(|(k, _)| match k {
                    Key::I(i) => Some(i),
                    _ => None,
                })
                .collect(),
            _ => return false,
        };
        pids.sort_unstable();
        oids.sort_unstable();
        if pids != oids {
            return false;
        }
    }
    true
}

/// PoB's closure `{"$fn": src}` against the port's `Value::JewelFunc` (exported as
/// `{ func = {"$fn": key}, … }`): the CalcSetup.lua:877-884 default is key `$default`, the
/// ModParser closures any other key.
fn jewel_func_equal(m: &Mismatch) -> bool {
    let pob = m.pob.as_ref().and_then(|p| p.get("$fn")).and_then(J::as_str);
    let ours = m.ours.as_ref().and_then(|o| o.get("func")).and_then(|f| f.get("$fn").or_else(|| f.get("func").and_then(|g| g.get("$fn")))).and_then(J::as_str);
    match (pob, ours) {
        (Some("Modules/CalcSetup.lua:877-884"), Some(k)) => k == "$default",
        (Some(p), Some(k)) => p.starts_with("Modules/ModParser.lua:") && k != "$default",
        _ => false,
    }
}

/// A ring's `slotModList[3]` (IT-C keeps it in `item.extra`, which the exporter does not write for
/// items the model loaded): same length as PoB's.
fn slot3_equal(build: &BuildRef, env: &EnvRef, pob_root: &J, m: &Mismatch) -> bool {
    let Some(prefix) = m.path.strip_suffix(".slotModList[3]") else { return false };
    // an empty Lua list dumps as `{}`
    let n = m.pob.as_ref().and_then(|v| v.as_array().map(Vec::len).or_else(|| v.as_object().filter(|o| o.is_empty()).map(|_| 0)));
    let item = match prefix.strip_prefix("env.player.itemList[\"").and_then(|r| r.strip_suffix("\"]")) {
        Some(slot) => env.player.itemList.with(|l| l.get(&Name::from(slot)).cloned()),
        None => {
            let name = at_path(pob_root, prefix).map(|v| resolve(pob_root, v)).and_then(|v| v.get("$item")).and_then(J::as_str);
            name.and_then(|name| build.itemsTab.items.with(|l| l.values().find(|i| &*i.name.get() == name).cloned()))
        }
    };
    let ours = item.map(|i| i.slotModList3().map_or(0, |l| l.len()));
    if std::env::var_os("CS2_DEBUG").is_some() {
        eprintln!("slot3 {}: pob {n:?} ours {ours:?}", m.path);
    }
    n.is_some() && n == ours
}

fn run_hit(
    build: &BuildRef,
    din: &StateDump,
    dout: Option<&StateDump>,
    inner_ejf: Option<usize>,
    stem: &str,
) -> Outcome {
    pob_calc::global_cache::clear_all();
    let inj = match catch(|| pob_oracle::inject::inject(din, build)) {
        Ok(Ok(i)) => i,
        Ok(Err(e)) => return Outcome::Error(format!("inject: {e}")),
        Err(e) => return Outcome::Error(format!("inject raised: {e}")),
    };
    let mut locals: Vec<(String, Val)> = inj.locals.into_iter().collect();
    let mut keep_alive: Vec<Val> = Vec::new();
    for (name, v) in &mut locals {
        if name == "override" && v.obj::<Override>().is_none() {
            if let Some(o) = as_override(v) {
                keep_alive.push(std::mem::replace(v, Val::Obj(o)));
            }
        }
    }
    // a second pass's `override.extraJewelFuncs` store has another env's player as its actor
    // (`$foreign`, a placeholder): give it a live actor (the injected env's player), as PoB's
    // first-pass player is alive there; EvalMod's ItemCondition needs `actor.itemList`
    if let Some(env) = inj.env.as_ref() {
        for (name, v) in &locals {
            if name == "override" {
                if let Some(o) = v.obj::<Override>() {
                    if let Some(l) = o.extraJewelFuncs.get() {
                        if l.actor.get().upgrade().is_none() {
                            l.actor.set(Rc::downgrade(&env.player));
                        }
                    }
                }
            }
        }
    }
    let before: std::collections::HashSet<String> =
        testkit::diff_against(din, inj.env.as_ref(), &locals, &format!("CS2-{stem}"))
            .iter()
            .map(key)
            .collect();
    let run = catch(|| pob_calc::probe::run_segment(&din.probe, &locals));
    let (flow, after) = match run {
        Ok(Ok(x)) => x,
        Ok(Err(e)) => return Outcome::Error(format!("run: {e}")),
        Err(e) => return Outcome::Error(format!("run raised: {e}")),
    };
    let ov = after
        .iter()
        .find(|(n, _)| n == "override")
        .and_then(|(_, v)| v.obj::<Override>());
    let Some(dout) = dout else {
        // PoB restarted at 1433
        if flow != Flow::Restart {
            return Outcome::RestartWrong(format!("PoB restarts, Rust returned {flow:?}"));
        }
        let ours = ov.and_then(|o| o.extraJewelFuncs.get()).map_or(0, |l| l.len());
        return match inner_ejf {
            Some(n) if n != ours => Outcome::RestartWrong(format!(
                "override.extraJewelFuncs: PoB {n} mods, Rust {ours}"
            )),
            _ => Outcome::Restart,
        };
    };
    if flow == Flow::Restart {
        return Outcome::RestartWrong("Rust restarts, PoB does not".to_owned());
    }
    let env = after
        .iter()
        .find(|(n, _)| n == "env")
        .and_then(|(_, v)| v.obj::<Env>())
        .or(inj.env);
    let Some(env) = env else {
        return Outcome::Error("no env after the run".to_owned());
    };
    // pre-load: PoB's model had no items yet
    let pob_items = dout
        .root
        .get("env")
        .and_then(|e| e.get("player"))
        .and_then(|p| p.get("itemList"))
        .and_then(J::as_object)
        .map_or(0, |o| o.keys().filter(|k| !k.starts_with('$')).count());
    let our_items = env.player.itemList.with(indexmap::IndexMap::len);
    if pob_items == 0 && our_items > 0 {
        return Outcome::PreLoad;
    }
    if let Some(d) = std::env::var_os("CS2_DUMP_OURS") {
        let ours = testkit::export_after(Some(&env), &after);
        let _ = std::fs::write(
            Path::new(&d).join(format!("{stem}-{}.json", din.hit.replace(':', "_"))),
            serde_json::to_string_pretty(&ours).unwrap_or_default(),
        );
    }
    let mm = testkit::diff_against(dout, Some(&env), &after, &format!("CS2-{stem}"));
    drop(keep_alive);
    // explodeSources: our Lua list (env.extra) against PoB's
    let pob_es = dout.root.get("env").and_then(|e| e.get("explodeSources"));
    let ours_es = {
        let t = env.extra.table(n!("explodeSources"));
        t.map(|t| {
            t.ipairs()
                .into_iter()
                .map(|(_, v)| {
                    if let Some(n) = v.obj::<Node>() {
                        format!("node {}", pob_lua::tostring_num(n.id))
                    } else if let Some(i) = v.obj::<Item>() {
                        format!("item {}", i.name.get())
                    } else {
                        format!("other {v:?}")
                    }
                })
                .collect::<Vec<_>>()
        })
        .unwrap_or_default()
    };
    let pob_es_ids = list_ids(&dout.root, pob_es);
    // LUA-ORDER: the node half follows `env.allocNodes`' layout (injected); the items keep order
    let split = |l: &[String]| {
        let mut nodes: Vec<String> = l.iter().filter(|x| x.starts_with("node ")).cloned().collect();
        nodes.sort();
        let rest: Vec<String> = l.iter().filter(|x| !x.starts_with("node ")).cloned().collect();
        (nodes, rest)
    };
    let es_equal = split(&pob_es_ids) == split(&ours_es);
    let ours_tree = testkit::export_after(Some(&env), &after);
    let radius_ok = radius_nodes_equal(&env, dout);
    let mut why: BTreeMap<&'static str, usize> = BTreeMap::new();
    let mut kept = Vec::new();
    for m in mm {
        let p = m.path.as_str();
        let reason: Option<&'static str> = if before.contains(&key(&m)) {
            Some("round-trip")
        } else if p.starts_with("env.explodeSources") && es_equal {
            Some("explodeSources (CCR-0030 #3)")
        } else if p.contains("$flaskSlotMap") || p.contains("$extraJewelFuncsActor") {
            Some("rust-only plumbing")
        } else if p.starts_with("globalCache") {
            Some("globalCache")
        } else if (p.starts_with("env.override.extraJewelFuncs") || p.starts_with("locals.override.extraJewelFuncs"))
            && din
                .root
                .get("locals")
                .and_then(|l| l.get("override"))
                .and_then(|o| o.get("extraJewelFuncs"))
                .is_some()
        {
            Some("override.extraJewelFuncs (second pass, $foreign actor)")
        } else if kind(&m) == "LuaOrder" && p.contains(".mods") && order_outside_tree_equal(&dout.root, &ours_tree, p) {
            Some("LUA-ORDER (tree-node mods: injected allocNodes layout)")
        } else if p.starts_with("env.radiusJewelList[") && p.contains("].nodes") && radius_ok {
            Some("radiusJewelList.nodes (node-id map exported as userdata; same ids)")
        } else if p.ends_with(".func") && jewel_func_equal(&m) {
            Some("jewel func (closure as data, calc doc §6)")
        } else if p.ends_with(".slotModList[3]") && kind(&m) == "Missing" && slot3_equal(build, &env, &dout.root, &m) {
            Some("slotModList[3] of model items (CCR-0061 §1 export)")
        } else if p.ends_with("[\"$slotModList3\"]") || p.ends_with(".$slotModList3") {
            Some("slotModList[3] of model items (CCR-0061 §1 export)")
        } else if p.ends_with(".mods.JewelFunc") && mods_equal_norm(&dout.root, &ours_tree, p) {
            Some("jewel func (closure as data, calc doc §6)")
        } else if kind(&m) == "Value"
            && m.pob.as_ref().zip(m.ours.as_ref()).is_some_and(|(a, b)| {
                ident(&dout.root, a).is_some() && ident(&dout.root, a) == ident(&ours_tree, b)
            })
        {
            Some("same object, other canonical path (explodeSources CCR-0030 #3)")
        } else if kind(&m) == "Value"
            && m.pob.as_ref().and_then(|v| v.get("$ref")).and_then(J::as_str).is_some_and(|r| {
                r.starts_with("env.override.extraJewelFuncs.actor") || r.starts_with("locals.override.extraJewelFuncs.actor")
            })
        {
            // a reused override's extraJewelFuncs store has an earlier env's player as its actor
            // (`$foreign` at injection): PoB's canonical path for that player's tables goes through it
            Some("override.extraJewelFuncs (second pass, $foreign actor)")
        } else if eager_field(&m) {
            Some("eager struct field")
        } else {
            None
        };
        match reason {
            Some(r) => *why.entry(r).or_default() += 1,
            None => kept.push(m),
        }
    }
    if !es_equal {
        kept.push(Mismatch {
            path: "env.extra.explodeSources (CCR-0030 #3)".to_owned(),
            kind: pob_oracle::diff::MismatchKind::Value,
            pob: Some(J::from(pob_es_ids.join(", "))),
            ours: Some(J::from(ours_es.join(", "))),
            writer: None,
        });
    }
    if kept.is_empty() {
        Outcome::Pass(why)
    } else {
        Outcome::Fail(
            kept.iter()
                .map(|m| format!("{} {:?}: pob={} ours={}", m.path, m.kind, short(&m.pob), short(&m.ours)))
                .collect(),
            why,
        )
    }
}

fn short(v: &Option<J>) -> String {
    let s = v.as_ref().map_or("-".to_owned(), J::to_string);
    if s.len() > 160 {
        format!("{}…", &s[..s.char_indices().nth(160).map_or(s.len(), |(i, _)| i)])
    } else {
        s
    }
}

fn path_shape(p: &str) -> String {
    let mut out = String::new();
    let mut skip = false;
    for c in p.chars() {
        match c {
            '[' => {
                skip = true;
                out.push_str("[]");
            }
            ']' => skip = false,
            _ if skip => {}
            _ => out.push(c),
        }
    }
    out.split('.').take(4).collect::<Vec<_>>().join(".")
}

fn load_model(xml: &Path) -> Result<BuildRef, String> {
    let text = std::fs::read_to_string(xml).map_err(|e| e.to_string())?;
    match catch(|| pob_calc::model::build_xml::loadBuildModelFromXML(&text, pob_gamedata::data())) {
        Ok(Ok(b)) => Ok(b),
        Ok(Err(e)) => Err(format!("load: {e}")),
        Err(e) => Err(format!("load raised: {e}")),
    }
}

/// Inject `din`, replace the `override` local by `ov` (keeping the injected `extraJewelFuncs`,
/// so the run does not restart), run the segment; the env afterwards.
fn run_with_override(build: &BuildRef, din: &StateDump, ov: Override) -> Result<EnvRef, String> {
    pob_calc::global_cache::clear_all();
    let inj = catch(|| pob_oracle::inject::inject(din, build))
        .map_err(|e| format!("inject raised: {e}"))?
        .map_err(|e| format!("inject: {e}"))?;
    let mut locals: Vec<(String, Val)> = inj.locals.into_iter().collect();
    let env = inj.env.clone().ok_or("no env")?;
    let mut kept = Vec::new();
    for (name, v) in &mut locals {
        if name == "override" {
            if let Some(o) = as_override(v) {
                ov.extraJewelFuncs.set(o.extraJewelFuncs.get());
            }
            if ov.extraJewelFuncs.get().is_none() {
                ov.extraJewelFuncs.set(Some(ModStore::ModList(None)));
            }
            kept.push(std::mem::replace(v, Val::Nil));
        }
    }
    let ov = Rc::new(ov);
    for (name, v) in &mut locals {
        if name == "override" {
            *v = Val::Obj(ov.clone());
        }
    }
    let (flow, after) = catch(|| pob_calc::probe::run_segment(&din.probe, &locals))
        .map_err(|e| format!("run raised: {e}"))??;
    if flow != Flow::Normal {
        return Err(format!("flow {flow:?}"));
    }
    drop(kept);
    Ok(after
        .iter()
        .find(|(n, _)| n == "env")
        .and_then(|(_, v)| v.obj::<Env>())
        .unwrap_or(env))
}

/// The 1425-1433 restart and the 905-931 extra-func pass (see `cs2_l4_overrides`).
fn restart_check(build: &BuildRef, din: &StateDump, ejf: &[ModRef]) -> Result<usize, String> {
    let run = |ov: Rc<Override>, add: bool| -> Result<(Flow, EnvRef), String> {
        pob_calc::global_cache::clear_all();
        let inj = catch(|| pob_oracle::inject::inject(din, build))
            .map_err(|e| format!("inject raised: {e}"))?
            .map_err(|e| format!("inject: {e}"))?;
        let env = inj.env.clone().ok_or("no env")?;
        if add {
            env.modDB.get().AddList(ejf);
        }
        let mut locals: Vec<(String, Val)> = inj.locals.into_iter().collect();
        for (name, v) in &mut locals {
            if name == "override" {
                *v = Val::Obj(ov.clone());
            }
        }
        let (flow, _) = catch(|| pob_calc::probe::run_segment(&din.probe, &locals))
            .map_err(|e| format!("run raised: {e}"))??;
        Ok((flow, env))
    };
    let ov = Rc::new(Override::default());
    let (flow, env1) = run(ov.clone(), true)?;
    if flow != Flow::Restart {
        return Err(format!("pass 1 returned {flow:?}"));
    }
    let l = ov.extraJewelFuncs.get().ok_or("override.extraJewelFuncs is nil")?;
    let names: Vec<Name> = l.ipairs().iter().map(|m| m.name).collect();
    if names != vec![n!("ExtraJewelFunc")] {
        return Err(format!("override.extraJewelFuncs holds {names:?}"));
    }
    drop(env1);
    if l.actor.get().upgrade().is_none() {
        return Err("override.extraJewelFuncs.actor died with the first env".to_owned());
    }
    // pass 2: the recursive call with the same override (Calcs.lua passes no specEnv here)
    let (flow, env2) = run(ov.clone(), false)?;
    if flow != Flow::Normal {
        return Err(format!("pass 2 returned {flow:?}"));
    }
    let extra = env2
        .radiusJewelList
        .get()
        .snapshot()
        .iter()
        .filter(|r| matches!(r.get(n!("func")), Val::Data(Value::JewelFunc(j)) if j.key.as_str() == "Modules/ModParser.lua:2630-2634"))
        .count();
    let want = env2.player.itemList.with(|l| {
        l.iter()
            .filter(|(k, i)| {
                k.as_str().starts_with("Jewel ")
                    && i.r#type.get() == n!("Jewel")
                    && i.rarity.get() != n!("UNIQUE")
                    && i.rarity.get() != n!("RELIC")
            })
            .count()
    });
    if extra != want {
        return Err(format!("pass 2: {extra} extra radius entries, {want} non-unique socketed jewels"));
    }
    Ok(extra)
}

/// `toggleFlask` (CalcSetup.lua 1368-1374) and `repSlotName` / `repItem` (815-820), driven on each
/// build's first MAIN hit after the load: the toggled flask's `env.flasks` entry flips, a
/// replaced slot holds `repItem`, and a two-handed `repItem` in "Weapon 1" removes "Weapon 2".
#[test]
fn cs2_l4_overrides() {
    let _serial = serial();
    let Some(_d) = pob_testpaths::require(pob_testpaths::dump_dir(), "data/dump") else {
        return;
    };
    assert!(testkit::init_default_data(), "data/dump does not load");
    let Some(pob_src) = pob_testpaths::require(pob_testpaths::pob_src(), "PoB checkout") else {
        return;
    };
    let Some(parity) = pob_testpaths::require(pob_testpaths::parity_dir(), ".parity") else {
        return;
    };
    quiet_lua_errors();
    setup_luts(&pob_src);
    let (mut flask_checks, mut rep_checks, mut staff_checks, mut restart_checks) = (0, 0, 0, 0);
    let mut restart_entries = 0;
    // an ExtraJewelFunc mod (ModParser.lua:2629-2634): the corpus has no build with one, so the
    // restart (1425-1433) is driven by adding it to `env.modDB` before the segment runs
    let (ejf, _) = pob_modparser::parseMod(
        "Non-Unique Jewels cause Small and Notable Passive Skills in a Large Radius to also grant +4 to Strength",
        false,
    );
    let ejf: Vec<ModRef> = ejf.unwrap_or_default().into_iter().map(Arc::new).collect();
    assert!(
        ejf.iter().any(|m| m.name == n!("ExtraJewelFunc")),
        "the ExtraJewelFunc line does not parse to an ExtraJewelFunc mod"
    );
    let mut failures = Vec::new();
    for xml in &builds(&parity) {
        let stem = stem(xml);
        let Ok(dir) = ensure_dumps(&pob_src, xml, &stem) else { continue };
        let Ok(build) = load_model(xml) else { continue };
        // the first MAIN hit whose PoB env has items (post-load) and that does not restart
        let state = dir.join("state");
        let outs = hit_files(&state, "initEnv:1451");
        let out_by_call: HashMap<String, PathBuf> =
            outs.values().filter_map(|p| call_of(p).map(|c| (c, p.clone()))).collect();
        let mut din = None;
        for p in hit_files(&state, "initEnv:800").values() {
            let Some(out) = call_of(p).and_then(|c| out_by_call.get(&c)).and_then(|q| StateDump::load(q).ok()) else {
                continue;
            };
            let items = out.root.get("env").and_then(|e| e.get("player")).and_then(|p| p.get("itemList"))
                .and_then(J::as_object).map_or(0, |o| o.keys().filter(|k| !k.starts_with('$')).count());
            let Ok(d) = StateDump::load(p) else { continue };
            let main = d.root.get("locals").and_then(|l| l.get("mode")).and_then(J::as_str) == Some("MAIN");
            if main && items > 0 {
                din = Some(d);
                break;
            }
        }
        let Some(din) = din else { continue };
        let base = match run_with_override(&build, &din, Override::default()) {
            Ok(e) => e,
            Err(e) => {
                failures.push(format!("{stem}: base run: {e}"));
                continue;
            }
        };
        // toggleFlask: every flask in a flask slot
        let slotted: Vec<(Name, ItemRef)> = build
            .itemsTab
            .orderedSlots
            .snapshot()
            .iter()
            .filter_map(|s| {
                let name = s.get(n!("slotName")).str()?;
                let id = s.get(n!("selItemId")).opt_num()?;
                let item = build.itemsTab.items.with(|m| m.get(&(id as u32)).cloned())?;
                Some((Name::from(&*name), item))
            })
            .collect();
        for (slot, item) in slotted.iter().filter(|(s, i)| s.as_str().starts_with("Flask ") && i.r#type.get() == n!("Flask")) {
            let was = base.flasks.get().with(|m| m.get(item)).unwrap_or(false);
            let ov = Override::default();
            ov.toggleFlask.set(Some(item.clone()));
            match run_with_override(&build, &din, ov) {
                Ok(e) => {
                    flask_checks += 1;
                    let now = e.flasks.get().with(|m| m.get(item)).unwrap_or(false);
                    if now == was {
                        failures.push(format!("{stem}: toggleFlask {slot}: env.flasks stayed {was}"));
                    }
                }
                Err(e) => failures.push(format!("{stem}: toggleFlask {slot}: {e}")),
            }
        }
        // repSlotName / repItem: put the Ring 1 item into Ring 2 (or the Amulet into itself)
        let ring1 = slotted.iter().find(|(s, _)| *s == n!("Ring 1")).map(|(_, i)| i.clone());
        if let Some(r1) = ring1 {
            let ov = Override::default();
            ov.repSlotName.set(Some(n!("Ring 2")));
            ov.repItem.set(Some(r1.clone()));
            match run_with_override(&build, &din, ov) {
                Ok(e) => {
                    rep_checks += 1;
                    let got = e.player.itemList.with(|m| m.get(&n!("Ring 2")).cloned());
                    if !got.is_some_and(|g| Rc::ptr_eq(&g, &r1)) {
                        failures.push(format!("{stem}: repItem Ring 2 is not the Ring 1 item"));
                    }
                }
                Err(e) => failures.push(format!("{stem}: repItem Ring 2: {e}")),
            }
        }
        // a two-handed repItem in Weapon 1 skips Weapon 2 (817-820)
        let staff = catch(|| pob_calc::classes::item::new_Item(Some("Rarity: NORMAL\nLong Staff\nLong Staff"), None, false));
        if let Some(staff) = staff.ok().filter(|s| s.base.get().is_some_and(|b| b.r#type == n!("Staff"))) {
            let ov = Override::default();
            ov.repSlotName.set(Some(n!("Weapon 1")));
            ov.repItem.set(Some(staff.clone()));
            match run_with_override(&build, &din, ov) {
                Ok(e) => {
                    staff_checks += 1;
                    let w1 = e.player.itemList.with(|m| m.get(&n!("Weapon 1")).cloned());
                    let w2 = e.player.itemList.with(|m| m.contains_key(&n!("Weapon 2")));
                    if !w1.is_some_and(|g| Rc::ptr_eq(&g, &staff)) || w2 {
                        failures.push(format!("{stem}: staff repItem: Weapon 1 not the staff or Weapon 2 kept"));
                    }
                }
                Err(e) => failures.push(format!("{stem}: staff repItem: {e}")),
            }
        }
        // the restart: pass 1 returns Flow::Restart with the mod in override.extraJewelFuncs (whose
        // actor stays alive); pass 2 (the same override) adds one radius entry per allocated
        // non-unique socketed jewel
        match restart_check(&build, &din, &ejf) {
            Ok(n) => {
                restart_checks += 1;
                restart_entries += n;
            }
            Err(e) => failures.push(format!("{stem}: restart: {e}")),
        }
        pob_calc::global_cache::clear_all();
        lut::clear_timeless_jewel_luts(None);
    }
    for f in &failures {
        eprintln!("  {f}");
    }
    eprintln!(
        "CS2 overrides: toggleFlask {flask_checks} runs, repItem {rep_checks} runs, two-handed repItem {staff_checks} runs, restart {restart_checks} builds ({restart_entries} extra-func radius entries in pass 2); {} failures",
        failures.len()
    );
    assert!(flask_checks + rep_checks + staff_checks > 0, "nothing checked");
    assert!(failures.is_empty(), "CS2 override checks failed");
}

#[derive(Default)]
struct Tally {
    pass: usize,
    fail: usize,
    restart: usize,
    restart_wrong: usize,
    preload: usize,
    errors: usize,
    explained: BTreeMap<&'static str, usize>,
    fail_paths: BTreeMap<String, usize>,
}

#[test]
fn cs2_l4_items_segment() {
    let _serial = serial();
    let Some(_d) = pob_testpaths::require(pob_testpaths::dump_dir(), "data/dump") else {
        return;
    };
    assert!(testkit::init_default_data(), "data/dump does not load");
    let Some(pob_src) = pob_testpaths::require(pob_testpaths::pob_src(), "PoB checkout") else {
        return;
    };
    let Some(parity) = pob_testpaths::require(pob_testpaths::parity_dir(), ".parity") else {
        return;
    };
    let corpus = builds(&parity);
    if pob_testpaths::require((!corpus.is_empty()).then_some(()), ".parity/builds/*.xml").is_none() {
        return;
    }
    quiet_lua_errors();
    setup_luts(&pob_src);
    let verbose = std::env::var_os("CS2_VERBOSE").is_some();
    let max_hits: usize = std::env::var("CS2_MAX_HITS").ok().and_then(|s| s.parse().ok()).unwrap_or(usize::MAX);
    let mut t = Tally::default();
    let mut lines = Vec::new();
    let mut missing = Vec::new();
    let t0 = std::time::Instant::now();
    for xml in &corpus {
        let stem = stem(xml);
        let dir = match ensure_dumps(&pob_src, xml, &stem) {
            Ok(d) => d,
            Err(e) => {
                missing.push(format!("{stem}: {e}"));
                continue;
            }
        };
        let build = match load_model(xml) {
            Ok(b) => b,
            Err(e) => {
                t.errors += 1;
                lines.push(format!("ERROR   {stem}: model {e}"));
                continue;
            }
        };
        let state = dir.join("state");
        let ins = hit_files(&state, "initEnv:800");
        let outs = hit_files(&state, "initEnv:1451");
        let mut out_by_call: HashMap<String, PathBuf> = HashMap::new();
        for p in outs.values() {
            if let Some(c) = call_of(p) {
                out_by_call.insert(c, p.clone());
            }
        }
        let calls: Vec<(u64, Option<String>)> = ins.iter().map(|(n, p)| (*n, call_of(p))).collect();
        for (i, (n, call)) in calls.iter().enumerate().take(max_hits) {
            let Ok(din) = StateDump::load(&ins[n]) else {
                t.errors += 1;
                lines.push(format!("ERROR   {stem} initEnv:800#{n}: unreadable"));
                continue;
            };
            let dout = call
                .as_ref()
                .and_then(|c| out_by_call.get(c))
                .and_then(|p| StateDump::load(p).ok());
            // a restart: the recursive call's own 800 hit carries the override PoB built
            let inner_ejf = if dout.is_none() {
                calls.get(i + 1).and_then(|(m, _)| StateDump::load(&ins[m]).ok()).and_then(|d| {
                    d.root
                        .get("locals")
                        .and_then(|l| l.get("override"))
                        .and_then(|o| o.get("extraJewelFuncs"))
                        .and_then(|s| s.get("mods"))
                        .and_then(J::as_object)
                        .map(|m| m.values().filter_map(J::as_array).map(Vec::len).sum())
                })
            } else {
                None
            };
            let mode = din
                .root
                .get("locals")
                .and_then(|l| l.get("mode"))
                .and_then(J::as_str)
                .unwrap_or("?")
                .to_owned();
            let tag = format!("{stem} initEnv:800#{n} ({mode})");
            match run_hit(&build, &din, dout.as_ref(), inner_ejf, &stem) {
                Outcome::Pass(why) => {
                    t.pass += 1;
                    for (k, v) in &why {
                        *t.explained.entry(k).or_default() += v;
                    }
                    if verbose {
                        lines.push(format!("ok      {tag} {why:?}"));
                    }
                }
                Outcome::Fail(mm, why) => {
                    t.fail += 1;
                    for (k, v) in &why {
                        *t.explained.entry(k).or_default() += v;
                    }
                    lines.push(format!("FAIL    {tag}: {} mismatches", mm.len()));
                    for l in &mm {
                        *t.fail_paths
                            .entry(path_shape(l.split(' ').next().unwrap_or("")))
                            .or_default() += 1;
                    }
                    for l in mm.iter().take(if verbose { usize::MAX } else { 6 }) {
                        lines.push(format!("          {l}"));
                    }
                }
                Outcome::Restart => {
                    t.restart += 1;
                    lines.push(format!("restart {tag}: Flow::Restart"));
                }
                Outcome::RestartWrong(e) => {
                    t.restart_wrong += 1;
                    lines.push(format!("RESTART {tag}: {e}"));
                }
                Outcome::PreLoad => t.preload += 1,
                Outcome::Error(e) => {
                    t.errors += 1;
                    lines.push(format!("ERROR   {tag}: {e}"));
                }
            }
        }
        drop(build);
        pob_calc::global_cache::clear_all();
        lut::clear_timeless_jewel_luts(None);
    }
    for l in &lines {
        eprintln!("{l}");
    }
    let compared = t.pass + t.fail;
    let pct = if compared == 0 { 0.0 } else { 100.0 * t.pass as f64 / compared as f64 };
    eprintln!(
        "CS2 L4 initEnv:800 -> initEnv:1451: pass {} / fail {} ({pct:.1}% of {compared} compared) / restart {} (wrong {}) / pre-load {} / errors {}; {} builds without dumps; {:.0}s",
        t.pass, t.fail, t.restart, t.restart_wrong, t.preload, t.errors, missing.len(), t0.elapsed().as_secs_f64()
    );
    eprintln!("  explained representation differences: {:?}", t.explained);
    let mut fp: Vec<(&String, &usize)> = t.fail_paths.iter().collect();
    fp.sort_by(|a, b| b.1.cmp(a.1));
    for (p, n) in fp.iter().take(40) {
        eprintln!("  mismatch path {p}: {n}");
    }
    for m in &missing {
        eprintln!("missing: {m}");
    }
    if !missing.is_empty() && pob_testpaths::require::<()>(None, &missing.join("; ")).is_none() {
        return;
    }
    let min_pct: f64 = std::env::var("CS2_MIN_PASS_PCT").ok().and_then(|s| s.parse().ok()).unwrap_or(98.0);
    assert_eq!(t.errors, 0, "CS2 L4: {} hits raised (see stderr)", t.errors);
    assert_eq!(t.restart_wrong, 0, "CS2 L4: {} restart mismatches", t.restart_wrong);
    assert!(compared > 0, "CS2 L4: nothing compared");
    assert!(pct >= min_pct, "CS2 L4: {pct:.1}% of compared hits identical < {min_pct}%");
}
