//! Unit PWR acceptance (calc doc §11.1 row PWR): CalcsTab.lua 441-773 (BuildOutput, BuildPower,
//! the PowerBuilder iterator, CalculatePowerStat, CalculateCombinedOffDefStat) and
//! `data.powerStatList.GetFromOutput` (Data.lua 131, 164-168, 186-212).
//!
//! * `pwr_l5_replay` (the gate: "node power values equal PoB's for 3 builds × all nodes,
//!   sequential mode"): PoB runs `calcsTab:PowerBuilder()` on a loaded build with its misc
//!   calculator wrapped to record every call (`corpus/PWR/pwr.lua`); the Rust PowerBuilder runs on
//!   the Rust build model of the same XML with PoB's base output, PoB's `grantedPassives` and a
//!   `calc_replay` hook that answers each `calcFunc(override, useFullDPS)` with PoB's recorded
//!   output for the same node set. Every spec node's `power` table (singleStat, pathPower,
//!   offence, defence, distance, masteryEffects), every `clusterNodeMap` node's `power`,
//!   and `powerMax` must equal PoB's, and the Rust run must make as many calc calls as PoB with
//!   none outside PoB's set (count and membership, not order: the order follows `pairs` over
//!   node-id maps, PoB's per process, §3.4; a single-node call may be answered with the output
//!   PoB computed for another node of the same modKey, since which node of a shared modKey fills
//!   the cache follows that order too; the run prints how many), for 3 builds × the power-stat modes
//!   (Offence/Defence and `Life` on every build; on the first one also `PhysicalTakenHit` (a
//!   transform), `FullDPS` and both with `nodePowerMaxDepth` 3), plus a derived fixture (the first
//!   build with every socket group in Full DPS: no corpus build uses it, so it is the only case
//!   where `useFullDPS` nil vs false changes the calc). This isolates PWR from the calc's own
//!   gaps (CS2's item stub etc.).
//! * `pwr_coroutine_steps` (Rust only, on the same replayed calls): `BuildPower` stepped once
//!   per "frame" with a clock that passes 101 ms per read yields after every node and ends with
//!   the same powers as the direct run, then clears `powerBuilder` and calls
//!   `powerBuilderCallback` once; progress values are monotone. With a clock that passes 1 ms per
//!   read plus 150 ms between frames, each resume does 101 nodes (L660 restarts `start` after the
//!   yield returns, so the frame gap is not counted).
//! * `pwr_l6_report` (end to end; gated exact since FUP2): the real Rust calculator on one build
//!   (Offence/Defence, all nodes), against PoB: every power value exact, none missing or extra,
//!   and the default build's count pinned (`L6_OCCVORTEX_KEYS`); plus parallel mode (D12)
//!   against the Rust sequential run, whose divergences are reported only.
//! * `pwr_stat_fns` (Rust only): GetFromOutput / CalculatePowerStat / CalculateCombinedOffDefStat
//!   / the transforms on hand-made outputs, and the transform closures' identity (dump hashes).
//!
//! PoB runs are cached in `target/tmp/pwr-probe/<stem>/<mode>.json`. Knobs: `PWR_BUILDS=a,b,c`,
//! `PWR_VERBOSE=1`, `PWR_L6_BUILD=stem`, `PWR_L6_DEPTH=n`. A missing input skips with a message
//! (`POB_ORACLE_REQUIRE=1` or `CI` fails instead).
#![allow(
    non_snake_case,
    clippy::many_single_char_names,
    clippy::type_complexity,
    clippy::single_match_else,
    clippy::explicit_auto_deref
)]

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::{Arc, Mutex};

use pob_calc::modules::power::{self, calc_replay, PowerOptions, PowerState};
use pob_calc::prelude::*;
use pob_gamedata::timeless_lut as lut;
use pob_oracle::oracle::{find_parity_dir, find_pob_src, find_up};
use serde_json::Value as J;

static SERIAL: Mutex<()> = Mutex::new(());

/// The three gate builds: an old tree without masteries, a 3.2x tree with cluster jewels and an
/// Intuitive-Leap-like jewel, and a minion build with cluster jewels.
const BUILDS: &[&str] = &["upstream_OccVortex", "allflame_exp_000", "allflame_exp_014"];

/// (mode name, power stat, nodePowerMaxDepth, only on the first build).
const MODES: &[(&str, Option<&str>, Option<f64>, bool)] = &[
    ("offdef", None, None, false),
    ("Life", Some("Life"), None, false),
    ("PhysicalTakenHit", Some("PhysicalTakenHit"), None, true),
    ("FullDPS", Some("FullDPS"), None, true),
    ("offdef", None, Some(3.0), true),
    ("Life", Some("Life"), Some(3.0), true),
    // (the Full DPS fixture runs offdef, Life and FullDPS)
];

fn skip(why: &str) {
    let required = std::env::var_os("POB_ORACLE_REQUIRE").is_some_and(|v| v == "1")
        || std::env::var_os("CI").is_some_and(|v| !v.is_empty() && v != "false" && v != "0");
    assert!(!required, "required input missing: {why}");
    eprintln!("SKIPPED: {why}");
}

fn verbose() -> bool {
    std::env::var_os("PWR_VERBOSE").is_some()
}

fn init_data() -> bool {
    if pob_gamedata::is_initialised() {
        return true;
    }
    let dir = if let Some(p) = std::env::var_os("POB_DUMP_DIR").map(PathBuf::from) {
        p
    } else if let Some(m) = find_up("data/dump/data.bin") {
        m.parent().map(Path::to_path_buf).unwrap_or_default()
    } else {
        return false;
    };
    pob_gamedata::init_from_dir(&dir).unwrap_or_else(|e| panic!("data.bin: {e}"));
    true
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
    match std::panic::catch_unwind(std::panic::AssertUnwindSafe(f)) {
        Ok(r) => Ok(r),
        Err(p) => Err(p
            .downcast_ref::<LuaError>()
            .map(|e| format!("Lua error: {}", e.0))
            .or_else(|| p.downcast_ref::<String>().map(|s| format!("panic: {s}")))
            .or_else(|| p.downcast_ref::<&str>().map(|s| format!("panic: {s}")))
            .unwrap_or_else(|| "panic".to_owned())),
    }
}

fn setup_luts() -> bool {
    let Some(src) = find_pob_src() else {
        return false;
    };
    let src = std::fs::canonicalize(&src).unwrap_or(src);
    lut::set_jewel_fs(Arc::new(lut::StdJewelFs {
        script_path: src.to_string_lossy().into_owned(),
        writable: false,
    }));
    lut::set_con_printf(|_| {});
    true
}

fn builds(parity: &Path) -> Vec<PathBuf> {
    let list: Vec<String> = match std::env::var("PWR_BUILDS") {
        Ok(v) => v.split(',').map(|s| s.trim().to_owned()).collect(),
        Err(_) => BUILDS.iter().map(|s| (*s).to_owned()).collect(),
    };
    list.iter()
        .map(|s| parity.join("builds").join(format!("{s}.xml")))
        .filter(|p| p.is_file())
        .collect()
}

fn stem(p: &Path) -> String {
    p.file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or_default()
        .to_owned()
}

// ---- values ------------------------------------------------------------------------------------

#[derive(Clone, Debug, PartialEq)]
enum Leaf {
    Num(f64),
    Bool(bool),
    Str(String),
}

/// Exact: the same double, or both NaN (±0 equal: CCR-0002).
fn leaf_exact(a: &Leaf, b: &Leaf) -> bool {
    match (a, b) {
        (Leaf::Num(x), Leaf::Num(y)) => x == y || (x.is_nan() && y.is_nan()),
        _ => a == b,
    }
}

/// The §9.2 tolerance.
fn leaf_close(a: &Leaf, b: &Leaf) -> bool {
    match (a, b) {
        (Leaf::Num(x), Leaf::Num(y)) => {
            (x.is_nan() && y.is_nan()) || x == y || (x - y).abs() <= 1e-9 * x.abs().max(1.0)
        }
        _ => a == b,
    }
}

fn json_num(j: &J) -> Option<f64> {
    if let Some(x) = j.as_f64() {
        return Some(x);
    }
    match j.get("$num").and_then(J::as_str) {
        Some("nan") => Some(f64::NAN),
        Some("inf") => Some(f64::INFINITY),
        Some("-inf") => Some(f64::NEG_INFINITY),
        _ => None,
    }
}

fn json_leaf(j: &J) -> Option<Leaf> {
    if let Some(x) = json_num(j) {
        return Some(Leaf::Num(x));
    }
    match j {
        J::Bool(b) => Some(Leaf::Bool(*b)),
        J::String(s) => Some(Leaf::Str(s.clone())),
        _ => None,
    }
}

fn flat_json(prefix: &str, j: &J, out: &mut BTreeMap<String, Leaf>) {
    if let Some(l) = json_leaf(j) {
        out.insert(prefix.to_owned(), l);
        return;
    }
    match j {
        J::Object(m) => {
            for (k, v) in m {
                let p = if prefix.is_empty() {
                    k.clone()
                } else {
                    format!("{prefix}.{k}")
                };
                flat_json(&p, v, out);
            }
        }
        J::Array(a) => {
            for (i, v) in a.iter().enumerate() {
                let p = if prefix.is_empty() {
                    (i + 1).to_string()
                } else {
                    format!("{prefix}.{}", i + 1)
                };
                flat_json(&p, v, out);
            }
        }
        _ => {}
    }
}

fn key_str(k: &Key) -> String {
    match k {
        Key::S(n) => n.to_string(),
        Key::I(i) => i.to_string(),
        Key::B(b) => b.to_string(),
        Key::F(f) => pob_lua::tostring_num(f.get()),
    }
}

fn flat_table(prefix: &str, t: &Table, out: &mut BTreeMap<String, Leaf>) {
    for (k, v) in t.pairs() {
        let ks = key_str(&k);
        let p = if prefix.is_empty() {
            ks
        } else {
            format!("{prefix}.{ks}")
        };
        match v {
            Val::Num(x) => {
                out.insert(p, Leaf::Num(x));
            }
            Val::Bool(b) => {
                out.insert(p, Leaf::Bool(b));
            }
            Val::Str(s) => {
                out.insert(p, Leaf::Str(s.to_string()));
            }
            Val::Table(t) => flat_table(&p, &t, out),
            _ => {}
        }
    }
}

fn val_of(j: &J) -> Val {
    match json_leaf(j) {
        Some(Leaf::Num(x)) => Val::Num(x),
        Some(Leaf::Bool(b)) => Val::Bool(b),
        Some(Leaf::Str(s)) => Val::from(s.as_str()),
        None => Val::Nil,
    }
}

/// A recorded slim output (`{ top = {…}, minion = {…} }`) as a calc output table.
fn output_of(j: &J) -> Output {
    let fill = |t: &Table, m: &J| {
        if let Some(m) = m.as_object() {
            for (k, v) in m {
                t.set(Name::from(k.as_str()), val_of(v));
            }
        }
    };
    let out = Table::new();
    fill(&out, &j["top"]);
    if j.get("minion").is_some_and(J::is_object) {
        let m = Table::new();
        fill(&m, &j["minion"]);
        out.set(n!("Minion"), m);
    }
    out
}

// ---- PoB side ----------------------------------------------------------------------------------

#[cfg(feature = "luajit")]
mod pob_side {
    use super::*;
    use pob_oracle::headless::Engine;
    use pob_oracle::jit::JitPob;
    use pob_oracle::oracle::ORACLE_LUA;

    const PWR_LUA: &str = include_str!("../corpus/PWR/pwr.lua");

    /// The Lua that selects one mode before `pwr.lua` runs.
    fn mode_lua(stat: Option<&str>, depth: Option<f64>) -> String {
        format!(
            "__pwr_stat = {}\n__pwr_depth = {}\n",
            stat.map_or("nil".to_owned(), |s| format!("{s:?}")),
            depth.map_or("nil".to_owned(), |d| d.to_string())
        )
    }

    /// PoB's power runs of one build: (mode, stat, depth) → the `pwr.lua` JSON. One PoB process
    /// per build (modes run in order on the same load). TP1: the whole run is one entry of the
    /// shared oracle query cache (`testkit::oracle_query`, keyed by the PoB tree, the XML, the
    /// build name, the scripts and the modes in order), so every worktree shares it.
    pub fn pob_runs(
        xml: &Path,
        modes: &[(&str, Option<&str>, Option<f64>)],
    ) -> Result<Vec<J>, String> {
        let st = stem(xml);
        let src = find_pob_src().ok_or("no PoB checkout")?;
        let xml_text = std::fs::read_to_string(xml).map_err(|e| e.to_string())?;
        let sets: Vec<String> = modes.iter().map(|(_, s, d)| mode_lua(*s, *d)).collect();
        let mut key: Vec<&str> = vec!["PWR pob_runs 1", &st, ORACLE_LUA, PWR_LUA];
        for ((m, _, _), set) in modes.iter().zip(&sets) {
            key.push(m);
            key.push(set);
        }
        let text = pob_oracle::testkit::oracle_query(&src, xml_text.as_bytes(), &key, || {
            let mut pob = JitPob::boot_oracle(&src, None)?;
            pob.run(ORACLE_LUA)?;
            pob.load_build_xml(&xml_text, &st)?;
            let mut out = Vec::new();
            for ((m, _, _), set) in modes.iter().zip(&sets) {
                pob.run(set)?;
                let t = std::time::Instant::now();
                let r = pob.run(PWR_LUA)?;
                let text = r.first().map(|r| r.text().to_owned()).unwrap_or_default();
                eprintln!("PoB {st} {m}: {:?}", t.elapsed());
                out.push(serde_json::from_str::<J>(&text).map_err(|e| format!("{m}: {e}"))?);
            }
            Ok(J::Array(out).to_string())
        })?;
        match serde_json::from_str::<J>(&text).map_err(|e| format!("cached PWR run: {e}"))? {
            J::Array(a) if a.len() == modes.len() => Ok(a),
            other => Err(format!(
                "cached PWR run: not {} modes: {other:.80}",
                modes.len()
            )),
        }
    }
}

// ---- Rust side ---------------------------------------------------------------------------------

fn rust_build(xml: &Path) -> Result<BuildRef, String> {
    let text = std::fs::read_to_string(xml).map_err(|e| e.to_string())?;
    pob_calc::global_cache::clear_all();
    let b = catch(|| pob_calc::model::build_xml::loadBuildFromXML(&text, pob_gamedata::data()))?;
    // the LUT table is process-global: dropped here only when no pool runs (`pwr_l5_replay`
    // drops it after its pool)
    pob_oracle::testkit::release_shared(|| lut::clear_timeless_jewel_luts(None));
    Ok(b)
}

/// A modKey with closure identities normalised: PoB's `tostring(func)` is an address
/// (`function: 0x…`, per process), the port's a hash of the closure's spec in the same shape
/// (`mod_tools::function_text`, FUP2; FOLLOWUPS "PWR" item 2).
fn norm_modKey(k: &str) -> String {
    let mut out = String::with_capacity(k.len());
    let mut rest = k;
    while let Some(i) = rest.find("function: ") {
        out.push_str(&rest[..i]);
        out.push_str("function");
        rest = &rest[i + "function: ".len()..];
        let end = rest
            .find(|c: char| !c.is_ascii_alphanumeric())
            .unwrap_or(rest.len());
        rest = &rest[end..];
    }
    out.push_str(rest);
    out
}

/// One recorded PoB call: its identity and output.
struct Call {
    remove: bool,
    nodes: Vec<(i64, String)>,
    useFullDPS: String,
    out: J,
}

fn calls_of(pob: &J) -> Vec<Call> {
    pob["calls"]
        .as_array()
        .map(|a| {
            a.iter()
                .map(|c| Call {
                    remove: c["remove"].as_bool().unwrap_or(false),
                    nodes: c["nodes"]
                        .as_array()
                        .map(|ns| {
                            ns.iter()
                                .map(|p| {
                                    (
                                        json_num(&p[0]).unwrap_or(f64::NAN) as i64,
                                        norm_modKey(p[1].as_str().unwrap_or_default()),
                                    )
                                })
                                .collect()
                        })
                        .map(|mut v: Vec<(i64, String)>| {
                            v.sort();
                            v
                        })
                        .unwrap_or_default(),
                    useFullDPS: c["useFullDPS"].as_str().unwrap_or("nil").to_owned(),
                    out: c["out"].clone(),
                })
                .collect()
        })
        .unwrap_or_default()
}

fn ufd_str(u: Option<bool>) -> &'static str {
    match u {
        None => "nil",
        Some(true) => "true",
        Some(false) => "false",
    }
}

/// The replay state: PoB's calls, the Rust calls made, misses.
#[derive(Default)]
struct Replay {
    calls: Vec<Call>,
    made: Vec<(bool, Vec<(i64, String)>, String)>,
    misses: Vec<String>,
    fallbacks: usize,
}

fn override_key(ov: &Override) -> (bool, Vec<(i64, String)>) {
    let (remove, set) = match (ov.addNodes.get(), ov.removeNodes.get()) {
        (Some(s), _) => (false, s),
        (None, Some(s)) => (true, s),
        (None, None) => return (false, Vec::new()),
    };
    let mut v: Vec<(i64, String)> = set
        .with(LjIntMap::pairs)
        .into_iter()
        .map(|(k, n)| (k, norm_modKey(&n.modKey.get())))
        .collect();
    v.sort();
    (remove, v)
}

fn install_replay(rep: &Rc<std::cell::RefCell<Replay>>) {
    let rep = rep.clone();
    calc_replay::set(Some(Rc::new(move |ov: &Override, ufd: Option<bool>| {
        let (remove, nodes) = override_key(ov);
        let ufd = ufd_str(ufd).to_owned();
        let mut r = rep.borrow_mut();
        r.made.push((remove, nodes.clone(), ufd.clone()));
        let hit = r
            .calls
            .iter()
            .position(|c| c.remove == remove && c.nodes == nodes && c.useFullDPS == ufd);
        let hit = hit.or_else(|| {
            // a single-node call PoB answered from its modKey cache with another node's output
            // (which node fills the cache follows PoB's per-process pairs order, §3.4)
            (nodes.len() == 1)
                .then(|| {
                    r.calls.iter().position(|c| {
                        c.remove == remove
                            && c.nodes.len() == 1
                            && c.nodes[0].1 == nodes[0].1
                            && c.useFullDPS == ufd
                    })
                })
                .flatten()
                .inspect(|_| r.fallbacks += 1)
        });
        match hit {
            Some(i) => Some(output_of(&r.calls[i].out)),
            None => {
                r.misses.push(format!(
                    "{} {:?} useFullDPS={ufd}",
                    if remove { "remove" } else { "add" },
                    nodes
                ));
                // the keys the power stats read, all zero
                let o = Table::new();
                for k in [
                    "LifeUnreserved",
                    "Life",
                    "Armour",
                    "EnergyShield",
                    "Evasion",
                    "LifeRegenRecovery",
                    "EnergyShieldRegenRecovery",
                    "CombinedDPS",
                ] {
                    o.set(Name::from(k), 0.0);
                }
                Some(o)
            }
        }
    })));
}

/// The Rust build prepared for a replay of `pob`: PoB's base output and grantedPassives, the
/// mode's power stat and depth.
fn prepare(build: &BuildRef, pob: &J, stat: Option<&str>, depth: Option<f64>) {
    let mainEnv = build.calcsTab.mainEnv.get().expect("mainEnv");
    let mut g = LjIntMap::new();
    for id in pob["granted"].as_array().into_iter().flatten() {
        if let Some(x) = json_num(id) {
            g.set(x as i64, Some(true));
        }
    }
    mainEnv.grantedPassives.set(Shared::new(g));
    let (f, _) = power::GetMiscCalculator(build);
    power::set_miscCalculator(build, Some((f, output_of(&pob["base"]))));
    power::set_powerStat(
        build,
        stat.map(|s| power::powerStat_by_stat(s).expect("powerStatList entry")),
    );
    power::set_nodePowerMaxDepth(build, depth);
}

/// Every spec node's and cluster node's `power`, and `powerMax`, flattened.
fn rust_powers(build: &BuildRef) -> BTreeMap<String, Leaf> {
    let mut out = BTreeMap::new();
    let spec = build.spec.get();
    for (id, node) in spec.nodes.with(LjIntMap::pairs) {
        if let Some(p) = node.extra.table(n!("power")) {
            flat_table(&format!("nodes.{id}.power"), &p, &mut out);
        }
    }
    let tree = spec.tree.get();
    for (k, v) in tree.clusterNodeMap.pairs() {
        if let Some(node) = v.obj::<Node>() {
            if let Some(p) = node.extra.table(n!("power")) {
                flat_table(&format!("cluster.{}.power", key_str(&k)), &p, &mut out);
            }
        }
    }
    if let Some(m) = power::powerMax(build) {
        flat_table("powerMax", &m, &mut out);
    }
    out
}

fn pob_powers(pob: &J) -> BTreeMap<String, Leaf> {
    let mut out = BTreeMap::new();
    if let Some(nodes) = pob["nodes"].as_object() {
        for (id, n) in nodes {
            flat_json(&format!("nodes.{id}.power"), &n["power"], &mut out);
        }
    }
    if let Some(c) = pob["cluster"].as_object() {
        for (name, n) in c {
            flat_json(&format!("cluster.{name}.power"), &n["power"], &mut out);
        }
    }
    flat_json("powerMax", &pob["powerMax"], &mut out);
    out
}

#[derive(Default, Debug)]
struct Cmp {
    keys: usize,
    exact: usize,
    close: usize,
    missing: usize,
    extra: usize,
    samples: Vec<String>,
}

fn compare(ours: &BTreeMap<String, Leaf>, pob: &BTreeMap<String, Leaf>) -> Cmp {
    let mut c = Cmp::default();
    for (k, p) in pob {
        c.keys += 1;
        match ours.get(k) {
            Some(o) if leaf_exact(o, p) => {
                c.exact += 1;
                c.close += 1;
            }
            Some(o) => {
                if leaf_close(o, p) {
                    c.close += 1;
                }
                if c.samples.len() < 12 {
                    c.samples.push(format!("{k}: Rust {o:?} vs PoB {p:?}"));
                }
            }
            None => {
                c.missing += 1;
                if c.samples.len() < 12 {
                    c.samples.push(format!("{k}: Rust missing, PoB {p:?}"));
                }
            }
        }
    }
    for (k, o) in ours {
        if !pob.contains_key(k) {
            c.extra += 1;
            c.keys += 1;
            if c.samples.len() < 12 {
                c.samples.push(format!("{k}: Rust {o:?}, PoB missing"));
            }
        }
    }
    c
}

/// Spec-model differences that explain power differences: alloc, modKey, pathDist per node.
fn model_diffs(build: &BuildRef, pob: &J) -> Vec<String> {
    let mut d = Vec::new();
    let spec = build.spec.get();
    if let Some(nodes) = pob["nodes"].as_object() {
        for (id, n) in nodes {
            let Ok(i) = id.parse::<i64>() else { continue };
            let Some(node) = spec.nodes.with(|m| m.get(i)) else {
                d.push(format!("node {id}: not in the Rust spec"));
                continue;
            };
            if n["alloc"].as_bool() != Some(node.alloc.get()) {
                d.push(format!(
                    "node {id}: alloc Rust {} PoB {}",
                    node.alloc.get(),
                    n["alloc"]
                ));
            }
            if n["modKey"].as_str().map(norm_modKey) != Some(norm_modKey(&node.modKey.get())) {
                d.push(format!("node {id}: modKey differs"));
            }
            if json_num(&n["pathDist"]) != node.pathDist.get() {
                d.push(format!(
                    "node {id}: pathDist Rust {:?} PoB {}",
                    node.pathDist.get(),
                    n["pathDist"]
                ));
            }
        }
        let rust_n = spec.nodes.with(LjIntMap::pairs).len();
        if rust_n != nodes.len() {
            d.push(format!("spec node count Rust {rust_n} PoB {}", nodes.len()));
        }
    }
    d
}

// ---- L5 replay (the gate) ----------------------------------------------------------------------

/// `<stem>_fullDPS.xml` in the test tmp dir: `xml` with `includeInFullDPS="true"` on every
/// socket group.
fn full_dps_fixture(xml: &Path) -> PathBuf {
    let text = std::fs::read_to_string(xml).expect("build xml");
    let mut out = String::with_capacity(text.len() + 1024);
    let mut rest = text.as_str();
    while let Some(i) = rest.find("<Skill ") {
        out.push_str(&rest[..i]);
        let end = rest[i..].find('>').map_or(rest.len(), |e| i + e);
        let tag = rest[i..end]
            .replace(" includeInFullDPS=\"nil\"", "")
            .replace(" includeInFullDPS=\"false\"", "")
            .replace(" includeInFullDPS=\"true\"", "");
        out.push_str(&tag.replacen("<Skill ", "<Skill includeInFullDPS=\"true\" ", 1));
        rest = &rest[end..];
    }
    out.push_str(rest);
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("pwr-fixtures");
    std::fs::create_dir_all(&dir).expect("fixture dir");
    let p = dir.join(format!("{}_fullDPS.xml", stem(xml)));
    std::fs::write(&p, out).expect("fixture");
    p
}

#[cfg(feature = "luajit")]
/// One build's PWR L5 results (TP1: builds run on the bounded pool, merged in corpus order).
#[derive(Default)]
struct L5Out {
    lines: Vec<String>,
    failures: Vec<String>,
    runs: usize,
    keys: usize,
    exact: usize,
    calls: usize,
}

fn l5_build(bi: usize, xml: &PathBuf, full: &PathBuf) -> L5Out {
    let mut o = L5Out::default();
    let st = stem(xml);
    let modes: Vec<(&str, Option<&str>, Option<f64>)> = MODES
        .iter()
        .filter(|(m, _, _, first)| {
            if xml == full {
                matches!(*m, "offdef" | "Life" | "FullDPS")
            } else {
                !first || bi == 0
            }
        })
        .map(|(m, s, d, _)| (*m, *s, *d))
        .collect();
    let pobs = pob_side::pob_runs(xml, &modes).unwrap_or_else(|e| panic!("PoB {st}: {e}"));
    let build = rust_build(xml).unwrap_or_else(|e| panic!("Rust load {st}: {e}"));
    for ((mode, stat, depth), pob) in modes.iter().zip(&pobs) {
        assert_eq!(pob["ok"], J::Bool(true), "PoB {st} {mode}: {}", pob["err"]);
        let model = model_diffs(&build, pob);
        prepare(&build, pob, *stat, *depth);
        let rep = Rc::new(std::cell::RefCell::new(Replay {
            calls: calls_of(pob),
            ..Replay::default()
        }));
        install_replay(&rep);
        let t = std::time::Instant::now();
        let r = catch(|| power::PowerBuilder_run(&build, &PowerOptions::default()));
        calc_replay::set(None);
        let elapsed = t.elapsed();
        let ours = rust_powers(&build);
        let theirs = pob_powers(pob);
        let c = compare(&ours, &theirs);
        let rep = rep.borrow();
        o.runs += 1;
        o.keys += c.keys;
        o.exact += c.exact;
        o.calls += rep.calls.len();
        o.lines.push(format!(
            "PWR L5 {st} {mode}{}: power values {}/{} exact ({} within 1e-9, {} missing, {} Rust-only); \
             calls Rust {} PoB {} ({} answered from another node of the same modKey, {} misses); \
             PoB {:.1} s, Rust {:.2?}; model diffs {}",
            depth.map_or(String::new(), |d| format!(" depth {d}")), c.exact, c.keys, c.close, c.missing, c.extra, rep.made.len(), rep.calls.len(), rep.fallbacks,
            rep.misses.len(), pob["seconds"].as_f64().unwrap_or(0.0), elapsed, model.len()
        ));
        if verbose() || c.exact != c.keys {
            for s in &c.samples {
                o.lines.push(format!("    {s}"));
            }
            for m in model.iter().take(8) {
                o.lines.push(format!("    model: {m}"));
            }
            for m in rep.misses.iter().take(4) {
                o.lines.push(format!("    miss: {m}"));
            }
        }
        if let Err(e) = r {
            o.failures.push(format!("{st} {mode}: Rust {e}"));
        }
        if c.exact != c.keys {
            o.failures.push(format!(
                "{st} {mode}: {} of {} power values differ",
                c.keys - c.exact,
                c.keys
            ));
        }
        if !rep.misses.is_empty() {
            o.failures.push(format!(
                "{st} {mode}: {} calc calls PoB never made",
                rep.misses.len()
            ));
        }
        if rep.made.len() != rep.calls.len() {
            o.failures.push(format!(
                "{st} {mode}: {} Rust calc calls vs {} PoB",
                rep.made.len(),
                rep.calls.len()
            ));
        }
    }
    drop(build);
    pob_calc::global_cache::clear_all();
    o
}

#[test]
fn pwr_l5_replay() {
    let _serial = SERIAL
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let Some(parity) = find_parity_dir() else {
        return skip(".parity not found");
    };
    if find_pob_src().is_none() {
        return skip("PoB checkout (.PathOfBuilding/src) not found");
    }
    if !init_data() {
        return skip("data/dump/data.bin not found");
    }
    quiet_lua_errors();
    setup_luts();
    let mut builds = builds(&parity);
    if builds.is_empty() {
        return skip(".parity/builds: none of the PWR builds found");
    }
    let gate_builds = builds.len();
    // No corpus build includes a skill in Full DPS, so `usedFullDPS` is false everywhere and
    // `useFullDPS` never matters: a derived fixture (the first build with every socket group
    // in Full DPS) covers the calcFullDPS path (nil vs false, CalcsTab.lua:498 / Calcs.lua:103).
    let full = full_dps_fixture(&builds[0]);
    builds.push(full.clone());
    let mut failures = Vec::new();
    let (mut total_keys, mut total_exact, mut runs, mut total_calls) = (0, 0, 0, 0);
    pob_oracle::testkit::par_for_each(
        &builds,
        |bi, xml| l5_build(bi, xml, &full),
        |_, _, o| {
            for l in &o.lines {
                eprintln!("{l}");
            }
            failures.extend(o.failures);
            runs += o.runs;
            total_keys += o.keys;
            total_exact += o.exact;
            total_calls += o.calls;
        },
    );
    lut::clear_timeless_jewel_luts(None);
    eprintln!("{}", pob_oracle::runner::query_cache_summary());
    eprintln!(
        "PWR L5: {runs} runs over {} builds, {total_exact}/{total_keys} power values exact, {total_calls} PoB calc calls replayed",
        builds.len()
    );
    // The quick corpus (14 builds) holds only one of PWR's gate builds.
    let min_gate = if pob_testpaths::quick_corpus() { 1 } else { 3 };
    assert!(
        gate_builds >= min_gate || std::env::var_os("PWR_BUILDS").is_some(),
        "fewer than {min_gate} gate builds found"
    );
    assert!(failures.is_empty(), "{failures:#?}");
}

// ---- the coroutine, stepped ---------------------------------------------------------------------

thread_local! {
    static CLOCK: std::cell::Cell<f64> = const { std::cell::Cell::new(0.0) };
}

/// A clock that passes 101 ms per read: the body yields after every counted node.
fn fast_clock() -> f64 {
    CLOCK.with(|c| {
        let t = c.get() + 101.0;
        c.set(t);
        t
    })
}

/// A clock that passes 1 ms per read; the frame loop adds [`FRAME_GAP`] between `BuildPower`
/// calls (the tree draw etc. between two PassiveTreeView.lua:785 calls).
fn slow_clock() -> f64 {
    CLOCK.with(|c| {
        let t = c.get() + 1.0;
        c.set(t);
        t
    })
}
const FRAME_GAP: f64 = 150.0;

/// `BuildPower` once per frame until the report ends (`gap` ms pass between frames) → frames.
fn run_frames(build: &BuildRef, gap: f64) -> usize {
    build.calcsTab.powerBuildFlag.set(true);
    let mut frames = 0usize;
    loop {
        power::BuildPower(build);
        frames += 1;
        if !power::powerBuilderRunning(build) {
            return frames;
        }
        CLOCK.with(|c| c.set(c.get() + gap));
        assert!(frames < 100_000, "the power builder never finished");
    }
}

#[cfg(feature = "luajit")]
#[test]
fn pwr_coroutine_steps() {
    let _serial = SERIAL
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let Some(parity) = find_parity_dir() else {
        return skip(".parity not found");
    };
    if find_pob_src().is_none() {
        return skip("PoB checkout (.PathOfBuilding/src) not found");
    }
    if !init_data() {
        return skip("data/dump/data.bin not found");
    }
    quiet_lua_errors();
    setup_luts();
    let Some(xml) = builds(&parity)
        .into_iter()
        .nth(1)
        .or_else(|| builds(&parity).into_iter().next())
    else {
        return skip(".parity/builds: none of the PWR builds found");
    };
    let st = stem(&xml);
    for (mode, stat) in [("offdef", None), ("Life", Some("Life"))] {
        let pob = pob_side::pob_runs(&xml, &[(mode, stat, None)])
            .unwrap_or_else(|e| panic!("PoB {st}: {e}"));
        let pob = &pob[0];
        let build = rust_build(&xml).unwrap_or_else(|e| panic!("Rust load {st}: {e}"));
        prepare(&build, pob, stat, None);
        let rep = Rc::new(std::cell::RefCell::new(Replay {
            calls: calls_of(pob),
            ..Replay::default()
        }));
        install_replay(&rep);
        // the direct run
        power::PowerBuilder_run(&build, &PowerOptions::default());
        let direct = rust_powers(&build);
        // the stepped run (BuildPower as PassiveTreeView calls it once per frame)
        let done = Rc::new(std::cell::Cell::new(0));
        let progress = Rc::new(std::cell::RefCell::new(Vec::<f64>::new()));
        {
            let done = done.clone();
            power::set_powerBuilderCallback(
                &build,
                Some(Rc::new(move || done.set(done.get() + 1))),
            );
            let progress = progress.clone();
            power::set_powerBuilderProgressCallback(
                &build,
                Some(Rc::new(move |p| progress.borrow_mut().push(p))),
            );
        }
        power::set_GetTime(Some(fast_clock));
        let frames = run_frames(&build, 0.0);
        let fast_progress = progress.borrow().clone();
        // the frame gap does not count against the next resume's 100 ms (L659-660: `start` is
        // read after `coroutine.yield()` returns). Each resume reads the clock once on resume and
        // once per counted node, so a resume runs 101 nodes (the last one yields); were `start`
        // read at the yield instead, every resume after the first time yield would run one node.
        progress.borrow_mut().clear();
        power::set_GetTime(Some(slow_clock));
        let gap_frames = run_frames(&build, FRAME_GAP);
        let gap_progress = progress.borrow().clone();
        *progress.borrow_mut() = fast_progress;
        // counted nodes: the fast clock yields after each one (frames = the L515 yield + one per
        // node + the resume that ends the body); with 1 ms per read and the clock restarted on
        // resume, a resume yields at its 101st node, so frames = 2 + nodes / 101
        let nodes = frames - 2;
        let want = 2 + nodes / 101;
        eprintln!(
            "PWR coroutine {st} {mode}: frame gap {FRAME_GAP} ms, 1 ms/read: {gap_frames} frames for {nodes} nodes (want {want}), {} progress callbacks",
            gap_progress.len()
        );
        assert_eq!(
            gap_frames, want,
            "about 100 ms of work per resume after a {FRAME_GAP} ms frame gap: {gap_frames} frames, want {want}"
        );
        assert_eq!(
            gap_progress.len(),
            gap_frames - 2,
            "one progress callback per time yield"
        );
        assert_eq!(done.get(), 2, "powerBuilderCallback once per run");
        done.set(1);
        power::set_GetTime(None);
        calc_replay::set(None);
        // during the run powerMax was the live table; at the end the final one
        assert!(power::powerMax(&build).is_some());
        assert!(power::powerBuilderInitialized(&build));
        let stepped = rust_powers(&build);
        let c = compare(&stepped, &direct);
        let p = progress.borrow();
        eprintln!(
            "PWR coroutine {st} {mode}: {frames} frames, {} progress callbacks, callback {}x; stepped vs direct {}/{} exact",
            p.len(), done.get(), c.exact, c.keys
        );
        for s in &c.samples {
            eprintln!("    {s}");
        }
        assert_eq!(done.get(), 1, "powerBuilderCallback once");
        assert!(
            frames > 100,
            "the body yields per node with the fast clock ({frames} frames)"
        );
        assert!(p.windows(2).all(|w| w[0] <= w[1]), "progress is monotone");
        assert!(
            p.iter().all(|x| (0.0..=100.0).contains(x)),
            "progress within 0..100"
        );
        assert_eq!(c.exact, c.keys, "the stepped run equals the direct run");
        // a second BuildPower without a new BuildOutput does nothing
        power::BuildPower(&build);
        assert_eq!(done.get(), 1);
        power::set_powerBuilderCallback(&build, None);
        power::set_powerBuilderProgressCallback(&build, None);
        pob_calc::global_cache::clear_all();
    }
    // a Lua error inside the body kills the coroutine (release PoB: not re-raised)
    let build = rust_build(&xml).unwrap_or_else(|e| panic!("Rust load {st}: {e}"));
    build.calcsTab.miscCalculator.set(None);
    build.calcsTab.powerBuildFlag.set(true);
    let cb = Rc::new(std::cell::Cell::new(0));
    {
        let cb = cb.clone();
        power::set_powerBuilderCallback(&build, Some(Rc::new(move || cb.set(cb.get() + 1))));
    }
    power::BuildPower(&build);
    assert!(
        !power::powerBuilderRunning(&build),
        "an error ends the coroutine"
    );
    assert_eq!(cb.get(), 1, "and its end calls the callback");
    // dev mode re-raises it
    power::set_launch_devMode(true);
    build.calcsTab.powerBuildFlag.set(true);
    let r = catch(|| power::BuildPower(&build));
    power::set_launch_devMode(false);
    assert!(
        r.is_err_and(|e| e.contains("miscCalculator")),
        "dev mode re-raises the error"
    );
    let _ = PowerState::default();
}

// ---- L6 (gated exact, FUP2) + parallel divergences (reported) ----------------------------------

#[cfg(feature = "luajit")]
#[test]
fn pwr_l6_report() {
    let _serial = SERIAL
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let Some(parity) = find_parity_dir() else {
        return skip(".parity not found");
    };
    if find_pob_src().is_none() {
        return skip("PoB checkout (.PathOfBuilding/src) not found");
    }
    if !init_data() {
        return skip("data/dump/data.bin not found");
    }
    quiet_lua_errors();
    setup_luts();
    let name = std::env::var("PWR_L6_BUILD").unwrap_or_else(|_| "upstream_OccVortex".to_owned());
    let xml = parity.join("builds").join(format!("{name}.xml"));
    if !xml.is_file() {
        return skip(&format!("{name}.xml not found"));
    }
    let depth: Option<f64> = std::env::var("PWR_L6_DEPTH")
        .ok()
        .and_then(|s| s.parse().ok());
    let pob = pob_side::pob_runs(&xml, &[("offdef", None, depth)])
        .unwrap_or_else(|e| panic!("PoB {name}: {e}"));
    let pob = &pob[0];
    let theirs = pob_powers(pob);

    // sequential, the real calculator
    let build = rust_build(&xml).unwrap_or_else(|e| panic!("Rust load {name}: {e}"));
    power::set_powerStat(&build, None);
    power::set_nodePowerMaxDepth(&build, depth);
    let t = std::time::Instant::now();
    let r = catch(|| power::PowerBuilder_run(&build, &PowerOptions::default()));
    let seq_time = t.elapsed();
    let seq = rust_powers(&build);
    let c = compare(&seq, &theirs);
    eprintln!(
"PWR L6 {name} offdef depth {depth:?}: {}/{} exact, {} within 1e-9 ({} missing, {} Rust-only); Rust {seq_time:.2?}, PoB {:.1} s{}",
        c.exact, c.keys, c.close, c.missing, c.extra, pob["seconds"].as_f64().unwrap_or(0.0),
        r.as_ref().err().map_or(String::new(), |e| format!("; Rust error: {e}"))
    );
    for s in c.samples.iter().take(6) {
        eprintln!("    {s}");
    }

    // parallel (D12): its own workers, against the Rust sequential run
    pob_calc::global_cache::clear_all();
    let build2 = rust_build(&xml).unwrap_or_else(|e| panic!("Rust load {name}: {e}"));
    power::set_powerStat(&build2, None);
    power::set_nodePowerMaxDepth(&build2, depth);
    let t = std::time::Instant::now();
    let r2 = catch(|| power::PowerBuilder_run(&build2, &PowerOptions { parallel: true }));
    let par_time = t.elapsed();
    let par = rust_powers(&build2);
    let d = compare(&par, &seq);
    eprintln!(
        "PWR parallel {name}: vs sequential {}/{} exact ({} within 1e-9); divergences {}; parallel {par_time:.2?} vs sequential {seq_time:.2?}{}",
        d.exact, d.keys, d.close, d.keys - d.exact,
        r2.as_ref().err().map_or(String::new(), |e| format!("; Rust error: {e}"))
    );
    for s in d.samples.iter().take(6) {
        eprintln!("    divergence: {s}");
    }
    // FUP2 (FOLLOWUPS "PWR" item 3): the sequential run is gated exact against PoB (CA's L6 is
    // exact on all57); parallel divergences stay reported only (D12). Neither run raises.
    assert!(r.is_ok() && r2.is_ok(), "{r:?} {r2:?}");
    assert!(
        c.exact == c.keys && c.missing == 0 && c.extra == 0,
        "PWR L6 {name}: {} of {} power values differ ({} missing, {} Rust-only): {:#?}",
        c.keys - c.exact,
        c.keys,
        c.missing,
        c.extra,
        c.samples
    );
    if name == "upstream_OccVortex" && depth.is_none() {
        assert_eq!(
            c.keys, L6_OCCVORTEX_KEYS,
            "PWR L6 {name}: compared power values"
        );
    }
}

/// Power values `pwr_l6_report` compares on its default build (upstream_OccVortex, Offence/Defence,
/// all nodes, no depth limit); measured by FUP2.
const L6_OCCVORTEX_KEYS: usize = 7672;

// ---- the stat functions (Rust only) -------------------------------------------------------------

#[test]
fn pwr_stat_fns() {
    if !init_data() {
        return skip("data/dump/data.bin not found");
    }
    let o = |kv: &[(&str, f64)], minion: &[(&str, f64)]| {
        let t = Table::new();
        for (k, v) in kv {
            t.set(Name::from(*k), *v);
        }
        if !minion.is_empty() {
            let m = Table::new();
            for (k, v) in minion {
                m.set(Name::from(*k), *v);
            }
            t.set(n!("Minion"), m);
        }
        t
    };
    let stat = |s: &str| power::powerStat_by_stat(s).expect("entry");
    // FullDPS: present, else CombinedDPS + Minion.CombinedDPS
    let a = o(
        &[("FullDPS", 10.0), ("CombinedDPS", 3.0)],
        &[("CombinedDPS", 4.0)],
    );
    let b = o(&[("CombinedDPS", 3.0)], &[("CombinedDPS", 4.0)]);
    assert_eq!(power::GetFromOutput(&a, &stat("FullDPS"), false), 10.0);
    assert_eq!(power::GetFromOutput(&b, &stat("FullDPS"), false), 7.0);
    assert_eq!(power::CalculatePowerStat(&stat("FullDPS"), &a, &b), 3.0);
    // damage stats add the minion's; other stats do not; Minion* reads the minion only
    let c = o(
        &[("TotalDPS", 2.0), ("Life", 100.0)],
        &[("TotalDPS", 5.0), ("Life", 50.0)],
    );
    assert_eq!(power::GetFromOutput(&c, &stat("TotalDPS"), false), 7.0);
    assert_eq!(power::GetFromOutput(&c, &stat("AverageDamage"), false), 0.0);
    assert_eq!(power::GetFromOutput(&c, &stat("Life"), false), 100.0);
    let ml = Table::new();
    ml.set(n!("stat"), "MinionLife");
    assert_eq!(power::GetFromOutput(&c, &ml, false), 50.0);
    // transforms: the *TakenHit stats negate (skipped with skipTransform)
    let d = o(&[("PhysicalTakenHit", 40.0)], &[]);
    assert_eq!(
        power::GetFromOutput(&d, &stat("PhysicalTakenHit"), false),
        -40.0
    );
    assert_eq!(
        power::GetFromOutput(&d, &stat("PhysicalTakenHit"), true),
        40.0
    );
    assert_eq!(
        power::powerStatTransform("data.powerStatList[2].transform", Val::from("The Shaper"))
            .str()
            .as_deref(),
        Some("Shaper")
    );
    // a nil stat raises (Lua: indexing the nil `stat`)
    let name = power::powerStat_table(&pob_gamedata::data().powerStatList.stats[1]);
    assert!(catch(|| power::GetFromOutput(&c, &name, false)).is_err());
    // the transform closures this port implements, by id and dump hash
    let mut seen = 0;
    for e in &pob_gamedata::data().powerStatList.stats {
        if let Some(f) = &e.transform {
            let want = match f.src.as_str() {
                "Modules/Data.lua:131-131" => {
                    "f1374b87c1d4bf47cf8cd4eb9e5c6a0bcbb69320f3f6f0f9a934441142aad86f"
                }
                "Modules/Data.lua:164-164" => {
                    "5dcffd8294ccd179ff647d4fffa4d1659eba00c472a4dd90f33a21f376ce36fd"
                }
                "Modules/Data.lua:165-165" => {
                    "e305ab055e0fabdb18075880f8742df93cb4259f0b2de4e4e32700e3ee56552f"
                }
                "Modules/Data.lua:166-166" => {
                    "464d0be7bfa3c46d5908329383db9d6f327ecd627e87d4e52abb70340fc0140c"
                }
                "Modules/Data.lua:167-167" => {
                    "3eafa4bf54d2f4b8b0d17c19963a42a6be76d42c44b001ee6c16b3b4044f3a40"
                }
                "Modules/Data.lua:168-168" => {
                    "f248ed21f926bc39295c5a8eae247ee39f17fc59b163c797020600de1137bbf6"
                }
                other => panic!("unknown transform {other}"),
            };
            assert_eq!(f.hash, want, "{}", f.id);
            // `value:gsub` on a number raises in LuaJIT (numbers have no string metatable)
            let r = catch(|| power::powerStatTransform(&f.id, Val::Num(1.0)));
            assert_eq!(
                r.is_err(),
                f.id == "data.powerStatList[2].transform",
                "{}",
                f.id
            );
            seen += 1;
        }
    }
    assert!(seen >= 6);
    // CalculateCombinedOffDefStat: both results, Lua's float order
    let base = o(
        &[
            ("LifeUnreserved", 4000.0),
            ("Life", 4000.0),
            ("Armour", 2000.0),
            ("EnergyShield", 100.0),
            ("Evasion", 500.0),
            ("LifeRegenRecovery", 50.0),
            ("EnergyShieldRegenRecovery", 0.0),
            ("CombinedDPS", 1000.0),
        ],
        &[("CombinedDPS", 500.0)],
    );
    let plus = o(
        &[
            ("LifeUnreserved", 4200.0),
            ("Life", 4200.0),
            ("Armour", 2000.0),
            ("EnergyShield", 100.0),
            ("EnergyShieldRecoveryCap", 80.0),
            ("Evasion", 500.0),
            ("LifeRegenRecovery", 60.0),
            ("EnergyShieldRegenRecovery", 0.0),
            ("CombinedDPS", 1100.0),
        ],
        &[],
    );
    let (off, def) = power::CalculateCombinedOffDefStat(&plus, &base);
    let want_def = (4200.0 - 4000.0) / 4000.0_f64.max(3000.0)
        + (2000.0 - 2000.0) / 10000.0
        + (80.0 - 100.0) / 3000.0
        + (500.0 - 500.0) / 10000.0
        + (60.0 - 50.0) / 500.0
        + 0.0 / 1000.0;
    assert_eq!(def, want_def);
    assert_eq!(off, (1100.0 + 0.0 - (1000.0 + 500.0)) / (1000.0 + 500.0));
    // a zero-DPS build: 0/0
    let z = o(
        &[
            ("LifeUnreserved", 1.0),
            ("Life", 1.0),
            ("Armour", 0.0),
            ("EnergyShield", 0.0),
            ("Evasion", 0.0),
            ("LifeRegenRecovery", 0.0),
            ("EnergyShieldRegenRecovery", 0.0),
            ("CombinedDPS", 0.0),
        ],
        &[],
    );
    assert!(power::CalculateCombinedOffDefStat(&z, &z).0.is_nan());
}
