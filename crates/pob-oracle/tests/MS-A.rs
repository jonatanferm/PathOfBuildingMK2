//! Unit MS-A acceptance (calc doc §11.1 row MS-A, §9.4 L3): the Rust ModStore / ModDB /
//! ModList against PoB's own, on the stores of every all57 build.
//!
//! For each build, PoB runs under LuaJIT (interpreter, one fresh VM per build, the machine-wide
//! oracle lock held) and `corpus/MS-A/l3_queries.lua` collects the main env's mod stores
//! (player / enemy / minion modDBs and their parents, every skillModList, item and buff mod
//! lists), serialises their mods in the dump shape, and runs the L3 query matrix with `pcall`:
//! every mod name (and name pairs / long name lists) × cfg ∈ {nil, skillCfg, weapon1Cfg,
//! weapon2Cfg, dotCfg, bleed/poison/ignite cfgs, two `source` cfgs, two flag cfgs} ×
//! {Sum BASE/INC, More, Flag, Override, List, Tabulate, HasMod, Max, Min}, plus GetCondition /
//! GetMultiplier (with and without noMod) for every condition / multiplier var and
//! GetStat for every output stat of each actor. It also runs ScaleAddMod on every player and
//! buff mod at six scales, and ScaleAddList with `replace` into a ModDB (twice: the second pass
//! meets the `replaced` marks), a ModList, a ModDB child of a ModDB parent (Replace parent
//! recursion) and a ModList child.
//!
//! The Rust side rebuilds the same stores (shared `Arc` mods, same parent chains, actors with
//! the dumped outputs), runs the same calls (LuaErrors caught), and compares.
//!
//! Gate (MS-A: "L3 for all non-tag paths"): every query whose names have no tagged mod in the
//! store chain matches, and every ScaleAddMod / ScaleAddList result matches. Queries over
//! tagged mods need EvalMod (MS-B); `Max`/`Min` call EvalMod on every mod (ModStore.lua:251/262),
//! so while EvalMod is still a stub they are reported, not gated (they are gated as soon as
//! EvalMod returns `mod.value` for an untagged mod).
//!
//! Knobs: `MS_A_BUILDS=stem1,stem2` or `MS_A_LIMIT=n` restrict the corpus; `MS_A_VERBOSE=1`
//! prints every mismatch. A missing input (data/dump, PoB checkout, .parity/builds) fails
//! (`pob_testpaths::require`; `POB_ALLOW_SKIP=1` skips instead).

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::Arc;

use pob_calc::prelude::*;
use pob_oracle::headless::{Engine, Ret};
use pob_oracle::jit::JitPob;
use pob_oracle::oracle::{find_parity_dir, find_pob_src, find_up};
use pob_oracle::runner::OracleLock;
use serde_json::Value as J;

const SCRIPT: &str = include_str!("../corpus/MS-A/l3_queries.lua");

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

/// `pob_lua::error::catch`: a LuaError panic as `Err` (other panics propagate).
fn catch<R>(f: impl FnOnce() -> R) -> Result<R, LuaError> {
    match std::panic::catch_unwind(std::panic::AssertUnwindSafe(f)) {
        Ok(r) => Ok(r),
        Err(p) => match p.downcast::<LuaError>() {
            Ok(e) => Err(*e),
            Err(other) => std::panic::resume_unwind(other),
        },
    }
}

/// LuaErrors are expected (the matrix provokes PoB's own errors): keep them off stderr.
fn quiet_lua_errors() {
    let prev = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        if info.payload().downcast_ref::<LuaError>().is_none() {
            prev(info);
        }
    }));
}

fn corpus(parity: &Path) -> Vec<PathBuf> {
    let dir = parity.join("builds");
    let mut v: Vec<PathBuf> = std::fs::read_dir(&dir)
        .map(|r| {
            r.filter_map(Result::ok)
                .map(|e| e.path())
                .filter(|p| p.extension().is_some_and(|x| x == "xml"))
                .collect()
        })
        .unwrap_or_default();
    v.sort();
    if let Ok(list) = std::env::var("MS_A_BUILDS") {
        let want: Vec<&str> = list.split(',').map(str::trim).collect();
        v.retain(|p| want.contains(&p.file_stem().unwrap().to_str().unwrap()));
    }
    if let Some(n) = std::env::var("MS_A_LIMIT")
        .ok()
        .and_then(|s| s.parse::<usize>().ok())
    {
        v.truncate(n);
    }
    v
}

// ---- decoding ----

/// A mod entry `{"mod": {…}, "marks": {…}}`: the mod plus the non-standard keys PoB wrote into
/// it (`replaced`, `converted`, …) in `extra`.
fn decode_mod(e: &J) -> Result<Mod, String> {
    let mut m: Mod = serde_json::from_value(e["mod"].clone()).map_err(|x| x.to_string())?;
    if let Some(marks) = e["marks"].as_object() {
        if !marks.is_empty() {
            let mut ex = m.extra.take().map(|b| *b).unwrap_or_default();
            for (k, v) in marks {
                let v: Value = serde_json::from_value(v.clone()).map_err(|x| x.to_string())?;
                ex.set(Name::from(k.as_str()), v);
            }
            m.extra = Some(Box::new(ex));
        }
    }
    Ok(m)
}

fn decode_value(v: &J) -> Value {
    serde_json::from_value(v.clone()).unwrap_or_else(|e| panic!("value {v}: {e}"))
}

fn num(v: &J) -> Option<f64> {
    match v {
        J::Number(n) => n.as_f64(),
        J::Object(o) => match o.get("$float").and_then(J::as_str) {
            Some("nan") => Some(f64::NAN),
            Some("inf") => Some(f64::INFINITY),
            Some("-inf") => Some(f64::NEG_INFINITY),
            Some("-0") => Some(-0.0),
            _ => None,
        },
        _ => None,
    }
}

fn same_num(a: f64, b: f64) -> bool {
    // ±0 is a host artefact on a dual-number LuaJIT (CCR-0002)
    a == b || (a.is_nan() && b.is_nan())
}

struct Build {
    mods: Vec<Option<ModRef>>,
    mod_id: HashMap<*const Mod, usize>,
    stores: Vec<ModStoreRef>,
    /// names with a tagged (or undecodable) mod anywhere in the store's chain
    tagged: Vec<std::collections::HashSet<Name>>,
    cfgs: Vec<Option<Cfg>>,
    _actors: Vec<ActorRef>,
    undecodable: usize,
    /// undecodable because of a tag key the frozen `Tag` census lacks (CCR-0014 §3)
    known_undecodable: usize,
    /// stores whose actor has no `activeSkillList` in Lua (the enemy; CCR-0014 §2)
    no_skill_list: std::collections::HashSet<usize>,
}

/// Tag keys PoB writes at runtime that the frozen `Tag` census lacks (CCR-0014 §3).
const KNOWN_TAG_GAPS: &[&str] = &["warcryPowerBonus"];

fn load(doc: &J) -> Build {
    let mut undecodable = 0;
    let mut known_undecodable = 0;
    let mut no_skill_list = std::collections::HashSet::new();
    let mut mods: Vec<Option<ModRef>> = Vec::new();
    let mut mod_id = HashMap::new();
    for (i, e) in doc["mods"].as_array().unwrap().iter().enumerate() {
        match decode_mod(e) {
            Ok(m) => {
                let m = Arc::new(m);
                mod_id.insert(Arc::as_ptr(&m), i + 1);
                mods.push(Some(m));
            }
            Err(err) => {
                if KNOWN_TAG_GAPS
                    .iter()
                    .any(|k| err.contains(&format!("\"{k}\"")))
                {
                    known_undecodable += 1;
                } else {
                    undecodable += 1;
                    if undecodable <= 3 {
                        eprintln!("  undecodable mod {}: {err}", i + 1);
                    }
                }
                mods.push(None);
            }
        }
    }
    let mod_at = |id: &J| -> Option<ModRef> { mods[id.as_u64().unwrap() as usize - 1].clone() };
    let sj = doc["stores"].as_array().unwrap();
    let stores: Vec<ModStoreRef> = sj
        .iter()
        .map(|s| match s["kind"].as_str().unwrap() {
            "ModDB" => ModStore::ModDB(None),
            _ => ModStore::ModList(None),
        })
        .collect();
    let mut own_tagged: Vec<std::collections::HashSet<Name>> = vec![Default::default(); sj.len()];
    for (i, s) in sj.iter().enumerate() {
        let store = &stores[i];
        if let Some(p) = s["parent"].as_u64() {
            store.parent.set(Some(stores[p as usize - 1].clone()));
        }
        for (k, v) in s["conditions"].as_object().unwrap() {
            store.conditions.set(Name::from(k.as_str()), json_val(v));
        }
        for (k, v) in s["multipliers"].as_object().unwrap() {
            store.multipliers.set(Name::from(k.as_str()), json_val(v));
        }
        let mut add = |name: Name, id: &J| match mod_at(id) {
            Some(m) => {
                assert_eq!(m.name, name, "mod stored under another name");
                if !m.tags.is_empty() {
                    own_tagged[i].insert(name);
                }
                store.AddMod(m);
            }
            None => {
                own_tagged[i].insert(name);
            }
        };
        if let Some(b) = s["buckets"].as_array() {
            for bucket in b {
                let name = Name::from(bucket[0].as_str().unwrap());
                for id in bucket[1].as_array().unwrap() {
                    add(name, id);
                }
            }
        } else {
            for id in s["list"].as_array().unwrap() {
                let name = mods[id.as_u64().unwrap() as usize - 1]
                    .as_ref()
                    .map_or_else(|| Name::from("?"), |m| m.name);
                add(name, id);
            }
        }
    }
    // chain-tagged names
    let mut tagged = Vec::new();
    for (i, s) in sj.iter().enumerate() {
        let mut set = own_tagged[i].clone();
        let mut p = s["parent"].as_u64();
        while let Some(pi) = p {
            set.extend(own_tagged[pi as usize - 1].iter().copied());
            p = sj[pi as usize - 1]["parent"].as_u64();
        }
        tagged.push(set);
    }
    // actors (GetStat)
    let mut actors = Vec::new();
    let skill_ge = pob_gamedata::data().skills.values().next().unwrap().clone();
    for (key, a) in doc["actors"].as_object().unwrap() {
        let actor = Actor::new();
        let out = actor.output.get();
        for (k, v) in a["output"].as_object().unwrap() {
            out.set(Name::from(k.as_str()), json_val(v));
        }
        if let Some(e) = num(&a["ManaEfficiency"]) {
            actor.extra.set(n!("ManaEfficiency"), e);
        }
        let list = actor.activeSkillList.get();
        for sk in a["skills"].as_array().unwrap() {
            let s = ActiveSkill::new(
                SkillEffect::new(skill_ge.clone(), 1.0, 0.0),
                Shared::new(Vec::new()),
                Rc::downgrade(&actor),
            );
            s.skillTypes.get().with_mut(|t| {
                t.set(
                    SkillType::HasReservation,
                    sk["hasReservation"].as_bool().unwrap(),
                );
            });
            s.skillFlags
                .get()
                .set(n!("disable"), sk["disable"].as_bool().unwrap());
            let bl = s.buffList.get();
            for bn in sk["buffNames"].as_array().unwrap() {
                let b = Table::new();
                if let Some(n) = bn.as_str() {
                    b.set(n!("name"), n);
                }
                bl.push(b);
            }
            for k in ["ManaReservedBase", "LifeReservedBase"] {
                if let Some(x) = num(&sk[k]) {
                    s.skillData.get().set(Name::from(k), x);
                }
            }
            list.push(s);
        }
        let has_list = a["hasSkillList"].as_bool().unwrap_or(true);
        for (i, s) in sj.iter().enumerate() {
            if s["actor"].as_str() == Some(key.as_str()) {
                stores[i].actor.set(Rc::downgrade(&actor));
                if !has_list {
                    no_skill_list.insert(i);
                }
            }
        }
        actors.push(actor);
    }
    // cfgs
    let cfgs = doc["cfgs"]
        .as_array()
        .unwrap()
        .iter()
        .map(|c| {
            if c["nil"].as_bool() == Some(true) {
                return None;
            }
            let cfg = ModCfg::new();
            if let Some(f) = num(&c["flags"]) {
                cfg.flags.set(f as u32);
            }
            if let Some(f) = num(&c["keywordFlags"]) {
                cfg.keywordFlags.set(f as u32);
            }
            cfg.source.set(c["source"].as_str().map(Name::from));
            cfg.skillName.set(c["skillName"].as_str().map(Name::from));
            cfg.summonSkillName
                .set(c["summonSkillName"].as_str().map(Name::from));
            if let Some(st) = c["skillStats"].as_object() {
                let t = Table::new();
                for (k, v) in st {
                    t.set(Name::from(k.as_str()), json_val(v));
                }
                cfg.skillStats.set(Some(t));
            }
            Some(cfg)
        })
        .collect();
    Build {
        mods,
        mod_id,
        stores,
        tagged,
        cfgs,
        _actors: actors,
        undecodable,
        known_undecodable,
        no_skill_list,
    }
}

fn json_val(v: &J) -> Val {
    match v {
        J::Bool(b) => Val::Bool(*b),
        J::String(s) => Val::Str(s.as_str().into()),
        other => num(other).map_or(Val::Nil, Val::Num),
    }
}

// ---- comparison ----

#[derive(Default)]
struct Tally {
    known: usize,
    gated: usize,
    gated_ok: usize,
    tag_path: usize,
    tag_ok: usize,
    eval_path: usize,
    eval_ok: usize,
    errors_both: usize,
    scale: usize,
    scale_ok: usize,
    scale_list: usize,
    scale_list_ok: usize,
    create_mod: usize,
    create_mod_ok: usize,
    /// PoB error text <-> Rust LuaError text, for the queries where both raise
    error_kinds: std::collections::BTreeMap<String, usize>,
    /// ScaleAddMod cases by the path they take (ModStore.lua 64-84), scale ≠ 1 only
    scale_paths: std::collections::BTreeMap<&'static str, usize>,
    mismatches: Vec<String>,
}

/// The ModStore.lua 64-84 branch a ScaleAddMod of `m` takes.
fn scale_path(m: &Mod) -> &'static str {
    if m.tags.iter().any(|t| t.unscalable) {
        return "unscalable";
    }
    let hp = |name: Name, ty: ModType| {
        pob_gamedata::data()
            .highPrecisionMods
            .get(&name)
            .is_some_and(|t| t.contains_key(&ty))
    };
    match &m.value {
        Value::Num(_) if hp(m.name, m.r#type) => "number, highPrecisionMods",
        Value::Num(v) if v.fract() != 0.0 => "number, defaultHighPrecision",
        Value::Num(_) => "number, round+modf",
        Value::Table(t) => match t.get(n!("mod")) {
            Value::Mod(sub) => match sub.value {
                Value::Num(_) if hp(sub.name, sub.r#type) => "value.mod number, highPrecisionMods",
                Value::Num(v) if v.fract() != 0.0 => "value.mod number, defaultHighPrecision",
                Value::Num(_) => "value.mod number, round+modf",
                _ => "value.mod non-number",
            },
            Value::Nil if t.get(n!("keyOfScaledMod")).truthy() => "keyOfScaledMod",
            _ => "other table",
        },
        _ => "other value",
    }
}

fn rust_query(
    b: &Build,
    store: &ModStore,
    q: &str,
    cfg: Option<&ModCfg>,
    names: &[Name],
) -> Result<J, String> {
    let r = catch(|| -> Result<J, String> {
        Ok(match q {
            "SumBASE" => num_j(store.Sum(ModType::BASE, cfg, names)),
            "SumINC" => num_j(store.Sum(ModType::INC, cfg, names)),
            "More" => num_j(store.More(cfg, names)),
            "Flag" => J::Bool(store.Flag(cfg, names)),
            "Override" => serde_json::to_value(store.Override(cfg, names)).unwrap(),
            "List" => J::Array(
                store
                    .List(cfg, names)
                    .iter()
                    .map(|v| serde_json::to_value(v).unwrap())
                    .collect(),
            ),
            "Tabulate" => J::Array(
                store
                    .Tabulate(None, cfg, names)
                    .iter()
                    .map(|r| {
                        let id = b.mod_id.get(&Arc::as_ptr(&r.r#mod)).copied().unwrap_or(0);
                        J::Array(vec![J::from(id), serde_json::to_value(&r.value).unwrap()])
                    })
                    .collect(),
            ),
            "HasModBASE" => J::Bool(store.HasMod(ModType::BASE, cfg, names)),
            "Max" => store.Max(cfg, names).map_or(J::Null, num_j),
            "Min" => store.Min(cfg, names).map_or(J::Null, num_j),
            "GetCondition" => J::Bool(store.GetCondition(names[0], cfg, false)),
            "GetConditionNoMod" => J::Bool(store.GetCondition(names[0], cfg, true)),
            "GetMultiplier" => num_j(store.GetMultiplier(names[0], cfg, false)),
            "GetMultiplierNoMod" => num_j(store.GetMultiplier(names[0], cfg, true)),
            "GetStat" => num_j(store.GetStat(names[0], cfg)),
            other => return Err(format!("unknown query {other}")),
        })
    });
    match r {
        Ok(v) => v,
        Err(e) => Err(e.0),
    }
}

fn num_j(x: f64) -> J {
    serde_json::to_value(Value::Num(x)).unwrap()
}

/// Lua result JSON vs Rust result JSON (both in the dump value shape).
fn same(q: &str, lua: &J, rust: &J) -> bool {
    match q {
        "Override" => decode_value(lua) == decode_value(rust),
        "List" => {
            let (Some(a), Some(b)) = (lua.as_array(), rust.as_array()) else {
                return false;
            };
            a.len() == b.len()
                && a.iter()
                    .zip(b)
                    .all(|(x, y)| decode_value(x) == decode_value(y))
        }
        "Tabulate" => {
            let (Some(a), Some(b)) = (lua.as_array(), rust.as_array()) else {
                return false;
            };
            a.len() == b.len()
                && a.iter()
                    .zip(b)
                    .all(|(x, y)| x[0] == y[0] && decode_value(&x[1]) == decode_value(&y[1]))
        }
        _ => match (num(lua), num(rust)) {
            (Some(a), Some(b)) => same_num(a, b),
            _ => lua == rust,
        },
    }
}

fn compare_queries(b: &Build, doc: &J, evalmod_ported: bool, t: &mut Tally, stem: &str) {
    for r in doc["results"].as_array().unwrap() {
        let si = r[0].as_u64().unwrap() as usize - 1;
        let q = r[1].as_str().unwrap();
        let ci = r[2].as_u64().unwrap() as usize - 1;
        let raw: Vec<Name> = r[3]
            .as_array()
            .unwrap()
            .iter()
            .map(|n| Name::from(n.as_str().unwrap()))
            .collect();
        let expect = &r[4];
        let store = &b.stores[si];
        let cfg = b.cfgs[ci].as_deref();
        // the mod names the query reads
        let read: Vec<Name> = match q {
            "GetCondition" | "GetConditionNoMod" => vec![conditionName(raw[0])],
            "GetMultiplier" | "GetMultiplierNoMod" => vec![multiplierName(raw[0])],
            "GetStat" => vec![],
            _ => raw.clone(),
        };
        let tag = read.iter().any(|n| b.tagged[si].contains(n));
        let eval = matches!(q, "Max" | "Min") && !evalmod_ported;
        let got = rust_query(b, store, q, cfg, &raw);
        // CCR-0014 §2: the Lua enemy actor has no activeSkillList (ipairs(nil) raises); the
        // frozen Rust Actor always has one.
        let known = q == "GetStat"
            && b.no_skill_list.contains(&si)
            && expect["err"]
                .as_str()
                .is_some_and(|e| e.contains("'ipairs'"));
        if known {
            t.known += 1;
            continue;
        }
        let ok = match (expect.get("err"), &got) {
            (Some(e), Err(r)) => {
                t.errors_both += 1;
                // "…/Classes/ModStore.lua:173: attempt to …" → "ModStore.lua:173: attempt to …"
                let e = e.as_str().unwrap_or("");
                let e = e.rsplit('/').next().unwrap_or(e);
                *t.error_kinds.entry(format!("{e}  <->  {r}")).or_default() += 1;
                true
            }
            (None, Ok(v)) => same(q, expect, v),
            _ => false,
        };
        if tag {
            t.tag_path += 1;
            t.tag_ok += usize::from(ok);
        } else if eval {
            t.eval_path += 1;
            t.eval_ok += usize::from(ok);
        } else {
            t.gated += 1;
            t.gated_ok += usize::from(ok);
            if !ok {
                t.mismatches.push(format!(
                    "{stem}: store {} ({}) {q} cfg {} {:?}: lua {} rust {:?}",
                    si + 1,
                    doc["stores"][si]["label"],
                    doc["cfgs"][ci]["name"],
                    raw.iter().map(|n| n.as_str()).collect::<Vec<_>>(),
                    expect,
                    got
                ));
            }
        }
    }
}

fn dump_store(store: &ModStore) -> Vec<(Name, Vec<Mod>)> {
    match store.kind {
        StoreKind::ModDB => store
            .mod_names()
            .into_iter()
            .map(|n| {
                (
                    n,
                    store.mods_named(n).iter().map(|m| (**m).clone()).collect(),
                )
            })
            .collect(),
        StoreKind::ModList => vec![(
            n!("$list"),
            store.ipairs().iter().map(|m| (**m).clone()).collect(),
        )],
    }
}

fn lua_store(v: &J) -> Option<Vec<(Name, Vec<Mod>)>> {
    if let Some(b) = v.get("buckets").and_then(J::as_array) {
        return Some(
            b.iter()
                .map(|x| {
                    (
                        Name::from(x[0].as_str().unwrap()),
                        x[1].as_array()
                            .unwrap()
                            .iter()
                            .map(|m| decode_mod(m).unwrap())
                            .collect(),
                    )
                })
                .collect(),
        );
    }
    let l = v.get("list")?.as_array()?;
    Some(vec![(
        n!("$list"),
        l.iter().map(|m| decode_mod(m).unwrap()).collect(),
    )])
}

fn compare_scaling(b: &Build, doc: &J, t: &mut Tally, stem: &str) {
    for r in doc["scale"].as_array().unwrap() {
        let id = r[0].as_u64().unwrap() as usize;
        let scale = num(&r[1]).unwrap();
        let Some(m) = b.mods[id - 1].clone() else {
            continue;
        };
        t.scale += 1;
        if scale != 1.0 {
            *t.scale_paths.entry(scale_path(&m)).or_default() += 1;
        }
        let ml = ModStore::ModList(None);
        let got = catch(|| ml.ScaleAddMod(&m, scale, false));
        let ok = match (&r[2], got) {
            (J::Object(o), Err(_)) if o.contains_key("err") => true,
            (J::Null, Ok(())) => ml.is_empty(),
            (e, Ok(())) if e.get("mod").is_some() => {
                let want = decode_mod(e).unwrap();
                ml.len() == 1 && *ml.ipairs()[0] == want
            }
            _ => false,
        };
        t.scale_ok += usize::from(ok);
        if !ok {
            t.mismatches.push(format!(
                "{stem}: ScaleAddMod(mod {id} {}, {scale}): lua {} rust {:?}",
                m.name,
                r[2],
                ml.ipairs().first().map(|m| m.value.clone())
            ));
        }
    }
    for r in doc["scaleList"].as_array().unwrap() {
        let ids: Vec<usize> = r["ids"]
            .as_array()
            .unwrap()
            .iter()
            .map(|x| x.as_u64().unwrap() as usize)
            .collect();
        if ids.iter().any(|&i| b.mods[i - 1].is_none()) {
            continue;
        }
        let plain: Vec<ModRef> = ids
            .iter()
            .map(|&i| b.mods[i - 1].clone().unwrap())
            .collect();
        let scale = num(&r["scale"]).unwrap();
        let db = ModStore::ModDB(None);
        db.AddList(&plain);
        let r1 = catch(|| {
            db.ScaleAddList(&plain, scale, true);
            db.ScaleAddList(&plain, scale, true);
        });
        let ml = ModStore::ModList(None);
        ml.AddList(&plain);
        let r2 = catch(|| ml.ScaleAddList(&plain, scale, true));
        let p = ModStore::ModDB(None);
        p.AddList(&plain);
        let c = ModStore::ModDB(Some(p.clone()));
        let r3 = catch(|| c.ScaleAddList(&plain, scale, true));
        let cl = ModStore::ModList(Some(p.clone()));
        let r4 = catch(|| cl.ScaleAddList(&plain, scale, false));
        for (what, res, store) in [
            ("db", r1.is_ok(), &db),
            ("ml", r2.is_ok(), &ml),
            ("parent", true, &p),
            ("child", r3.is_ok(), &c),
            ("childList", r4.is_ok(), &cl),
        ] {
            t.scale_list += 1;
            let lua = &r[what];
            let ok = match (lua_store(lua), res) {
                (None, false) => lua.as_str().is_some_and(|s| s.starts_with("err")),
                (Some(want), true) => want == dump_store(store),
                _ => false,
            };
            t.scale_list_ok += usize::from(ok);
            if !ok {
                t.mismatches.push(format!(
                    "{stem}: ScaleAddList {what} scale {scale} over {} mods differs",
                    ids.len()
                ));
            }
        }
    }
}

/// NewMod / ReplaceMod / ConvertMod / MergeNewMod scenarios (the Lua side's `createMod`
/// section) on the build's own untagged numeric mods. Needs `modLib.createMod` and
/// `compareModParams` (unit CT); `ported` says whether they are.
fn compare_create_mod(b: &Build, doc: &J, t: &mut Tally, stem: &str) {
    use pob_gamedata::mod_tools::createMod;
    let picked: Vec<ModRef> = doc["picked"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|id| b.mods[id.as_u64().unwrap() as usize - 1].clone())
        .collect();
    if picked.len() != doc["picked"].as_array().unwrap().len() {
        return;
    }
    for r in doc["createMod"].as_array().unwrap() {
        let k = r["pick"].as_u64().unwrap() as usize;
        let m = picked[k - 1].clone();
        let p = ModStore::ModDB(None);
        p.AddList(&picked);
        let c = ModStore::ModDB(Some(p.clone()));
        let l = ModStore::ModList(Some(p.clone()));
        let v = m.value.num().unwrap();
        let src = m.source.as_deref().unwrap();
        let (name, ty, fl, kw) = (m.name, m.r#type, m.flags, m.keywordFlags);
        let res = catch(|| {
            c.ReplaceMod(name, ty, v + 1.0, src, fl, kw, &[]);
            c.ReplaceMod(name, ty, v + 2.0, src, fl, kw, &[]);
            c.ConvertMod(
                name,
                Name::from(format!("{name}Conv").as_str()),
                ty,
                v * 2.0,
                src,
                fl,
                kw,
                &[],
            );
            l.NewMod(name, ty, 5.0, src, fl, kw, &[]);
            l.ConvertMod(
                name,
                Name::from(format!("{name}Conv2").as_str()),
                ty,
                3.0,
                src,
                fl,
                kw,
                &[],
            );
            l.NewMod(name, ty, 5.0, src, fl, kw, &[]);
            l.MergeNewMod(createMod(name, ty, 7.0, Some(src), fl, kw, &[]));
            l.MergeNewMod(createMod(
                Name::from(format!("{name}X").as_str()),
                ModType::BASE,
                1.0,
                Some(src),
                0,
                0,
                &[],
            ));
            l.ReplaceMod(name, ty, 9.0, src, fl, kw, &[]);
        });
        for (what, store) in [("parent", &p), ("child", &c), ("list", &l)] {
            t.create_mod += 1;
            let ok = r["err"].is_null() == res.is_ok()
                && lua_store(&r[what]).is_some_and(|want| want == dump_store(store));
            t.create_mod_ok += usize::from(ok);
            if !ok {
                t.mismatches.push(format!(
                    "{stem}: createMod scenario {k} ({name}) {what} differs"
                ));
            }
        }
    }
}

#[test]
fn l3_query_matrix_and_scaling_all57() {
    let Some(src) = pob_testpaths::require(
        find_pob_src(),
        "PoB checkout (.PathOfBuilding/src) not found",
    ) else {
        return; // POB_ALLOW_SKIP=1
    };
    let Some(parity) = pob_testpaths::require(find_parity_dir(), ".parity not found") else {
        return; // POB_ALLOW_SKIP=1
    };
    if pob_testpaths::require(init_data().then_some(()), "data/dump not found").is_none() {
        return; // POB_ALLOW_SKIP=1
    }
    let builds = corpus(&parity);
    if builds.is_empty() {
        let _ = pob_testpaths::require(None::<()>, ".parity/builds is empty");
        return; // POB_ALLOW_SKIP=1
    }
    quiet_lua_errors();
    let verbose = std::env::var_os("MS_A_VERBOSE").is_some();
    // EvalMod (MS-B) ported? For an untagged mod it returns `mod.value`.
    let evalmod_ported = {
        let probe = Mod {
            name: n!("MsAProbe"),
            r#type: ModType::MAX,
            value: Value::Num(7.0),
            flags: 0,
            keywordFlags: 0,
            source: None,
            tags: smallvec::SmallVec::new(),
            extra: None,
        };
        let db = ModStore::ModDB(None);
        catch(|| db.EvalMod(&probe, None, None)).ok() == Some(Value::Num(7.0))
    };
    // modLib.createMod / compareModParams (unit CT) ported? NewMod, ReplaceMod, ConvertMod and
    // MergeNewMod build their mod through them.
    let ct_ported = {
        let m = pob_gamedata::mod_tools::createMod(
            n!("MsAProbe"),
            ModType::BASE,
            1.0,
            Some("S"),
            0,
            0,
            &[],
        );
        m.value == Value::Num(1.0)
            && m.source.as_deref() == Some("S")
            && pob_gamedata::mod_tools::compareModParams(&m, &m)
    };
    let mut total = Tally::default();
    let mut stores = 0;
    let mut undecodable = 0;
    let mut known_undecodable = 0;
    let mut nbuilds = 0;
    for xml in &builds {
        let stem = xml.file_stem().unwrap().to_string_lossy().into_owned();
        let text = std::fs::read_to_string(xml).unwrap();
        let json = {
            let _lock = OracleLock::acquire().expect("oracle lock");
            let mut pob = JitPob::boot_oracle(&src, None).unwrap_or_else(|e| panic!("boot: {e}"));
            pob.load_build_xml(&text, &stem)
                .unwrap_or_else(|e| panic!("{stem}: load: {e}"));
            match pob
                .run(SCRIPT)
                .unwrap_or_else(|e| panic!("{stem}: script: {e}"))
                .first()
            {
                Some(Ret::Str(s)) => s.clone(),
                other => panic!("{stem}: script returned {other:?}"),
            }
        };
        let doc: J = serde_json::from_str(&json).unwrap_or_else(|e| panic!("{stem}: json: {e}"));
        let b = load(&doc);
        let mut t = Tally::default();
        compare_queries(&b, &doc, evalmod_ported, &mut t, &stem);
        compare_scaling(&b, &doc, &mut t, &stem);
        if ct_ported {
            compare_create_mod(&b, &doc, &mut t, &stem);
        }
        eprintln!(
            "{stem}: {} stores, {} mods ({} undecodable); gated {}/{}; tag-path {}/{}; Max/Min {}/{}; both-error {}; ScaleAddMod {}/{}; ScaleAddList {}/{}",
            b.stores.len(),
            b.mods.len(),
            b.undecodable,
            t.gated_ok,
            t.gated,
            t.tag_ok,
            t.tag_path,
            t.eval_ok,
            t.eval_path,
            t.errors_both,
            t.scale_ok,
            t.scale,
            t.scale_list_ok,
            t.scale_list
        );
        let shown = if verbose { t.mismatches.len() } else { 5 };
        for m in t.mismatches.iter().take(shown) {
            eprintln!("  MISMATCH {m}");
        }
        stores += b.stores.len();
        undecodable += b.undecodable;
        nbuilds += 1;
        total.known += t.known;
        known_undecodable += b.known_undecodable;
        total.gated += t.gated;
        total.gated_ok += t.gated_ok;
        total.tag_path += t.tag_path;
        total.tag_ok += t.tag_ok;
        total.eval_path += t.eval_path;
        total.eval_ok += t.eval_ok;
        total.errors_both += t.errors_both;
        total.scale += t.scale;
        total.scale_ok += t.scale_ok;
        total.scale_list += t.scale_list;
        total.scale_list_ok += t.scale_list_ok;
        total.create_mod += t.create_mod;
        total.create_mod_ok += t.create_mod_ok;
        for (k, v) in t.error_kinds {
            *total.error_kinds.entry(k).or_default() += v;
        }
        for (k, v) in t.scale_paths {
            *total.scale_paths.entry(k).or_default() += v;
        }
        total.mismatches.extend(t.mismatches);
    }
    eprintln!(
        "MS-A L3 over {nbuilds} builds, {stores} stores ({undecodable} undecodable mods, {known_undecodable} more with CCR-0014 tag keys):\n  \
         known diffs (CCR-0014: enemy activeSkillList) {}\n  \
         non-tag queries {}/{} identical ({} of them PoB errors reproduced)\n  \
         tag-path queries (EvalMod, MS-B; not gated) {}/{}\n  \
         Max/Min (EvalMod {}; {}) {}/{}\n  \
         ScaleAddMod {}/{}\n  ScaleAddList(replace) stores {}/{}",
        total.known,
        total.gated_ok,
        total.gated,
        total.errors_both,
        total.tag_ok,
        total.tag_path,
        if evalmod_ported { "ported" } else { "still a stub" },
        if evalmod_ported { "gated" } else { "not gated" },
        total.eval_ok,
        total.eval_path,
        total.scale_ok,
        total.scale,
        total.scale_list_ok,
        total.scale_list
    );
    eprintln!("  ScaleAddMod paths (scale != 1): {:?}", total.scale_paths);
    eprintln!("  errors raised by both (PoB <-> Rust):");
    for (k, v) in &total.error_kinds {
        eprintln!("    {v:>7}  {k}");
    }
    if ct_ported {
        eprintln!(
            "  NewMod/ReplaceMod/ConvertMod/MergeNewMod scenario stores {}/{}",
            total.create_mod_ok, total.create_mod
        );
    } else {
        eprintln!("  NewMod/ReplaceMod/ConvertMod/MergeNewMod: not run (createMod/compareModParams are CT stubs)");
    }
    assert_eq!(undecodable, 0, "mods the Rust model cannot decode");
    assert!(total.gated > 0);
    assert_eq!(total.gated_ok, total.gated, "non-tag L3 queries differ");
    assert_eq!(total.scale_ok, total.scale, "ScaleAddMod results differ");
    assert_eq!(
        total.scale_list_ok, total.scale_list,
        "ScaleAddList results differ"
    );
    if evalmod_ported {
        assert_eq!(total.eval_ok, total.eval_path, "Max/Min differ");
    }
    assert_eq!(
        total.create_mod_ok, total.create_mod,
        "createMod scenarios differ"
    );
    // coverage and the ungated buckets (HYG2/V2). MS-B (EvalMod) and CT (createMod) are merged,
    // so the stub fallbacks above are dead and must stay so; the tag-path queries are MS-B's
    // gate (MS-B.rs), here their misses are only bounded.
    assert!(
        evalmod_ported && ct_ported,
        "EvalMod / createMod are stubs again"
    );
    let filtered =
        std::env::var_os("MS_A_BUILDS").is_some() || std::env::var_os("MS_A_LIMIT").is_some();
    if !filtered {
        assert_eq!(nbuilds, CORPUS_BUILDS, "builds compared");
        let b = MS_A_BOUNDS;
        assert!(
            total.gated >= b.min_gated,
            "{} non-tag queries, expected at least {}",
            total.gated,
            b.min_gated
        );
        assert!(
            total.known <= b.max_known
                && known_undecodable <= b.max_known_undecodable
                && total.errors_both <= b.max_errors_both
                && total.tag_path - total.tag_ok <= b.max_tag_path_misses,
            "buckets: CCR-0014 known {} / CCR-0014 undecodable {known_undecodable} / both-error {} / \
             tag-path misses {}; bounds {b:?}",
            total.known,
            total.errors_both,
            total.tag_path - total.tag_ok
        );
    }
}

/// The corpus size (`.parity/builds/*.xml`).
const CORPUS_BUILDS: usize = 57;

#[derive(Debug)]
struct MsABounds {
    min_gated: usize,
    /// CCR-0014 known diffs (enemy activeSkillList).
    max_known: usize,
    /// Mods with CCR-0014 tag keys the Rust model does not decode.
    max_known_undecodable: usize,
    /// Errors PoB and the port both raise.
    max_errors_both: usize,
    /// Tag-path queries that differ (MS-B's gate; ungated here).
    max_tag_path_misses: usize,
}

/// Measured by HYG2 on rust-port 746b103.
const MS_A_BOUNDS: MsABounds = MsABounds {
    min_gated: 3_946_278,
    max_known: 964,
    max_known_undecodable: 4,
    max_errors_both: 33_771,
    // HYG2 exposed: 6771 of 1,326,872 tag-path queries differ (docs/port/FOLLOWUPS.md)
    max_tag_path_misses: 6771,
};
