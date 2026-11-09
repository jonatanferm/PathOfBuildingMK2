//! Unit MS-B acceptance (calc doc §11.1 row MS-B, §9.4 L3, §5.3):
//!
//! 1. `l3_full_matrix_all57`: the full L3 query matrix on the stores of every all57 build,
//!    i.e. MS-A's matrix with every tag path of `ModStore:EvalMod` gated. PoB runs under LuaJIT
//!    (interpreter, one fresh VM per build, the oracle lock held); `corpus/MS-B/l3_full.lua`
//!    collects the main env's mod stores **and everything EvalMod reads besides them** (the
//!    actor graph with links, weaponData, itemList items, minionData, outputs, and cfgs with
//!    skillCond/skillTypes/skillGrantedEffect/skillGem/skillPart/skillDist/slot and socket
//!    fields/item/baseFlags), then runs every mod name × cfg × {Sum BASE/INC, More, Flag,
//!    Override, List, Tabulate, HasMod, Max, Min} plus GetCondition / GetMultiplier / GetStat with
//!    `pcall`. `corpus/MS-B/rebuild.rs` rebuilds all of it in Rust; every query is compared.
//!    Gate: 100% of the queries match, except the documented known differences (counted and
//!    printed): CCR-0014 §2 (enemy `activeSkillList`), CCR-0015 §3 (Lua `nil` actor fields the
//!    frozen `Actor` cannot express), and queries that reach `Item:FindModifierSubstring` while
//!    it is still IT-B's stub (detected through the stub counter).
//! 2. `tag_div_no_reader`: the proof that no later reader observes Lua's `tag.div` write
//!    (ModStore.lua:401/499), which the port computes into a local (§5.3; CCR-0015 §1).
//!
//! Knobs: `MS_B_BUILDS=stem1,stem2` or `MS_B_LIMIT=n` restrict the corpus; `MS_B_VERBOSE=1`
//! prints every mismatch. A missing input (data/dump, PoB checkout, .parity/builds) fails
//! (`pob_testpaths::require`; `POB_ALLOW_SKIP=1` skips instead).
#![allow(non_snake_case)]

use std::path::{Path, PathBuf};
use std::sync::Arc;

use pob_calc::prelude::*;
use pob_oracle::headless::{Engine, Ret};
use pob_oracle::jit::JitPob;
use pob_oracle::oracle::{find_parity_dir, find_pob_src};
use pob_oracle::runner::OracleLock;
use serde_json::Value as J;

#[path = "../corpus/MS-B/rebuild.rs"]
mod rebuild;
use rebuild::{catch, decode_value, init_data, load, num, Build};

const FIND_MOD_SUBSTRING: &str = "Classes/Item.lua:289-333";

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
    if let Ok(list) = std::env::var("MS_B_BUILDS") {
        let want: Vec<&str> = list.split(',').map(str::trim).collect();
        v.retain(|p| want.contains(&p.file_stem().unwrap().to_str().unwrap()));
    }
    if let Some(n) = std::env::var("MS_B_LIMIT")
        .ok()
        .and_then(|s| s.parse::<usize>().ok())
    {
        v.truncate(n);
    }
    v
}

fn stub_hits(site: &str) -> u64 {
    pob_calc::stub::stub_report()
        .iter()
        .find(|(s, _)| *s == site)
        .map_or(0, |(_, c)| *c)
}

fn num_j(x: f64) -> J {
    serde_json::to_value(Value::Num(x)).unwrap()
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

fn same_num(a: f64, b: f64) -> bool {
    // ±0 is a host artefact on a dual-number LuaJIT (CCR-0002)
    a == b || (a.is_nan() && b.is_nan())
}

/// Structural equality of mod values with numbers compared like `same_num` (NaN equals NaN,
/// ±0 tolerated).
fn value_eq(a: &Value, b: &Value) -> bool {
    match (a, b) {
        (Value::Num(x), Value::Num(y)) => same_num(*x, *y),
        (Value::Table(x), Value::Table(y)) => {
            x.array.len() == y.array.len()
                && x.fields.len() == y.fields.len()
                && x.array.iter().zip(&y.array).all(|(p, q)| value_eq(p, q))
                && x.fields
                    .iter()
                    .zip(&y.fields)
                    .all(|((kp, p), (kq, q))| kp == kq && value_eq(p, q))
        }
        (Value::Mod(x), Value::Mod(y)) => {
            x.name == y.name
                && x.r#type == y.r#type
                && x.flags == y.flags
                && x.keywordFlags == y.keywordFlags
                && x.source == y.source
                && x.tags == y.tags
                && x.extra == y.extra
                && value_eq(&x.value, &y.value)
        }
        _ => a == b,
    }
}

fn same(q: &str, lua: &J, rust: &J) -> bool {
    match q {
        "Override" => value_eq(&decode_value(lua), &decode_value(rust)),
        "List" => {
            let (Some(a), Some(b)) = (lua.as_array(), rust.as_array()) else {
                return false;
            };
            a.len() == b.len()
                && a.iter()
                    .zip(b)
                    .all(|(x, y)| value_eq(&decode_value(x), &decode_value(y)))
        }
        "Tabulate" => {
            let (Some(a), Some(b)) = (lua.as_array(), rust.as_array()) else {
                return false;
            };
            a.len() == b.len()
                && a.iter().zip(b).all(|(x, y)| {
                    x[0] == y[0] && value_eq(&decode_value(&x[1]), &decode_value(&y[1]))
                })
        }
        _ => match (num(lua), num(rust)) {
            (Some(a), Some(b)) => same_num(a, b),
            _ => lua == rust,
        },
    }
}

#[derive(Default)]
struct Tally {
    total: usize,
    ok: usize,
    errors_both: usize,
    known_ccr0014: usize,
    known_ccr0015: usize,
    pending_itb: usize,
    undecodable: usize,
    by_query: std::collections::BTreeMap<String, (usize, usize)>,
    error_kinds: std::collections::BTreeMap<String, usize>,
    mismatches: Vec<String>,
}

fn compare(b: &Build, doc: &J, t: &mut Tally, stem: &str) {
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
        let read: Vec<Name> = match q {
            "GetCondition" | "GetConditionNoMod" => vec![conditionName(raw[0])],
            "GetMultiplier" | "GetMultiplierNoMod" => vec![multiplierName(raw[0])],
            _ => raw.clone(),
        };
        // A mod the Rust model cannot decode (CCR-0014 §3) is missing from the store.
        if read.iter().any(|n| b.undecodable_names[si].contains(n)) {
            t.undecodable += 1;
            continue;
        }
        let stub0 = stub_hits(FIND_MOD_SUBSTRING);
        let got = rust_query(b, store, q, cfg, &raw);
        let stubbed = stub_hits(FIND_MOD_SUBSTRING) != stub0;
        let lua_err = expect.get("err").and_then(J::as_str);
        let ok = match (lua_err, &got) {
            (Some(e), Err(r)) => {
                t.errors_both += 1;
                let e = e.rsplit('/').next().unwrap_or(e);
                *t.error_kinds.entry(format!("{e}  <->  {r}")).or_default() += 1;
                true
            }
            (None, Ok(v)) => same(q, expect, v),
            _ => false,
        };
        if !ok && stubbed {
            t.pending_itb += 1;
            continue;
        }
        let known = if ok {
            None
        } else {
            known_diff(b, si, lua_err, b.monster_tag_chain[si])
        };
        if let Some(k) = known {
            if k == "CCR-0014" {
                t.known_ccr0014 += 1;
            } else {
                t.known_ccr0015 += 1;
            }
            if std::env::var_os("MS_B_VERBOSE").is_some() {
                eprintln!(
                    "  KNOWN({k}) {stem}: store {} {q} {:?}: lua {expect} rust {got:?}",
                    b.labels[si],
                    raw.iter().map(|n| n.as_str()).collect::<Vec<_>>()
                );
            }
            continue;
        }
        t.total += 1;
        t.ok += usize::from(ok);
        let e = t.by_query.entry(q.to_owned()).or_default();
        e.0 += 1;
        e.1 += usize::from(ok);
        if !ok {
            t.mismatches.push(format!(
                "{stem}: store {} ({}) {q} cfg {} {:?}: lua {} rust {:?}",
                si + 1,
                b.labels[si],
                b.cfg_names[ci],
                raw.iter().map(|n| n.as_str()).collect::<Vec<_>>(),
                expect,
                got
            ));
        }
    }
}

/// `tag.type == "MonsterTag"`.
fn is_monster_tag(t: &Tag) -> bool {
    t.r#type.as_str() == "MonsterTag"
}

/// The documented known differences: Lua reads an actor field that is nil for this actor (the
/// enemy has no `activeSkillList` / `itemList`; GuardianRelicAll's minionData has no
/// `monsterTags`; an actor without `output`), which the frozen Rust types always hold as a
/// (possibly empty) table. CCR-0014 §2 and CCR-0015 §3. Each case needs the Lua error of that
/// nil read, except the `monsterTags` one (Lua returns nil there, no error), which needs a
/// MonsterTag-tagged mod that the query can evaluate (`monster`: one in the store's chain or the
/// probe mod).
fn known_diff(b: &Build, si: usize, lua_err: Option<&str>, monster: bool) -> Option<&'static str> {
    if let Some(e) = lua_err {
        if b.no_skill_list.contains(&si) && e.contains("'ipairs'") {
            return Some("CCR-0014");
        }
        if b.no_item_list.contains(&si) && e.contains("'pairs'") {
            return Some("CCR-0015");
        }
        if b.no_output.contains(&si) && e.contains("'output'") {
            return Some("CCR-0015");
        }
    }
    if monster && b.no_monster_tags.contains(&si) {
        return Some("CCR-0015");
    }
    None
}

#[derive(Default)]
struct Synth {
    tags: usize,
    probes: usize,
    undecodable_probes: usize,
    known_undecodable_probes: usize,
    total: usize,
    ok: usize,
    errors_both: usize,
    pending_itb: usize,
    known_ccr0014: usize,
    known_ccr0015: usize,
    by_tag: std::collections::BTreeMap<String, (usize, usize)>,
    mismatches: Vec<String>,
}

/// The synthetic tag probes (`__msb_synthetic`): every distinct tag of PoB's parser cache and
/// skill data on a probe mod in a ModDB child of the player / enemy / minion modDB.
fn compare_synth(b: &Build, doc: &J, s: &mut Synth, stem: &str) {
    let probes: Vec<Option<(String, ModRef)>> = doc["probes"]
        .as_array()
        .unwrap()
        .iter()
        .map(|p| match rebuild::decode_mod(&p["mod"]) {
            Ok(m) => Some((p["kind"].as_str().unwrap().to_owned(), Arc::new(m))),
            Err(e) => {
                if rebuild::KNOWN_TAG_GAPS
                    .iter()
                    .any(|k| e.contains(&format!("\"{k}\"")))
                {
                    // CCR-0014 §3: a warcry tag that CalcPerform.lua:2382 wrote into (shared data)
                    s.known_undecodable_probes += 1;
                } else {
                    s.undecodable_probes += 1;
                    eprintln!("  {stem}: undecodable probe: {e}");
                }
                None
            }
        })
        .collect();
    s.probes += probes.len();
    s.tags += probes
        .iter()
        .filter(|p| p.as_ref().is_some_and(|(k, _)| k == "BASE"))
        .count();
    let mut child: Option<(usize, usize, ModStoreRef)> = None;
    for r in doc["synth"].as_array().unwrap() {
        // store 0: a ModDB made without a parent (no actor)
        let base = r[0].as_u64().unwrap() as usize;
        let si = base.wrapping_sub(1);
        let label = if base == 0 {
            "no parent"
        } else {
            b.labels[si].as_str()
        };
        let pi = r[1].as_u64().unwrap() as usize - 1;
        let ci = r[2].as_u64().unwrap() as usize - 1;
        let expect = &r[3];
        let Some((kind, m)) = &probes[pi] else {
            continue;
        };
        // one ModDB child per (store, probe), as on the Lua side
        if child.as_ref().is_none_or(|(a, p, _)| *a != si || *p != pi) {
            let db = ModStore::ModDB((base != 0).then(|| b.stores[si].clone()));
            db.AddMod(m.clone());
            child = Some((si, pi, db));
        }
        let db = &child.as_ref().unwrap().2;
        let cfg = b.cfgs[ci].as_deref();
        let stub0 = stub_hits(FIND_MOD_SUBSTRING);
        let got = if kind == "BASE" {
            rust_query(b, db, "SumBASE", cfg, &[n!("MSBProbe")])
        } else {
            rust_query(b, db, "List", cfg, &[n!("MSBProbeL")])
        };
        let stubbed = stub_hits(FIND_MOD_SUBSTRING) != stub0;
        let lua_err = expect.get("err").and_then(J::as_str);
        let ok = match (lua_err, &got) {
            (Some(_), Err(_)) => {
                s.errors_both += 1;
                true
            }
            (None, Ok(v)) => same(if kind == "BASE" { "SumBASE" } else { "List" }, expect, v),
            _ => false,
        };
        if !ok && stubbed {
            s.pending_itb += 1;
            continue;
        }
        // The no-parent store (`base == 0`) has no carve-out: its `GetStat` is guarded in Lua
        // (ModStore.lua:355) and raises only for the three reservation stats, as in MS-A.
        let known = if ok || base == 0 {
            None
        } else {
            let monster = b.monster_tag_chain[si] || m.tags.iter().any(is_monster_tag);
            known_diff(b, si, lua_err, monster)
        };
        if let Some(k) = known {
            if k == "CCR-0014" {
                s.known_ccr0014 += 1;
            } else {
                s.known_ccr0015 += 1;
            }
            if std::env::var_os("MS_B_VERBOSE").is_some() {
                eprintln!(
                    "  SYNTH KNOWN({k}) {stem}: {label} {:?}: lua {expect} rust {got:?}",
                    m.tags[0]
                );
            }
            continue;
        }
        s.total += 1;
        s.ok += usize::from(ok);
        let tt = m.tags[0].r#type.as_str().to_owned();
        let e = s.by_tag.entry(tt).or_default();
        e.0 += 1;
        e.1 += usize::from(ok);
        if !ok {
            s.mismatches.push(format!(
                "{stem}: probe {} on {} cfg {}: tag {:?}: lua {} rust {:?}",
                pi + 1,
                label,
                b.cfg_names[ci],
                m.tags[0],
                expect,
                got
            ));
        }
    }
}

#[test]
fn l3_full_matrix_all57() {
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
    let verbose = std::env::var_os("MS_B_VERBOSE").is_some();
    let mut total = Tally::default();
    let mut nstores = 0;
    let mut nactors = 0;
    let mut nitems = 0;
    let mut ncfgs = 0;
    let mut undecodable = 0;
    let mut known_undecodable = 0;
    let mut ge_copies = 0;
    let mut tag_types: std::collections::BTreeMap<String, usize> = Default::default();
    let mut synth = Synth::default();
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
                .run(&format!("__msb_synthetic = true\n{}", rebuild::SCRIPT))
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
        compare(&b, &doc, &mut t, &stem);
        let mut sy = Synth::default();
        compare_synth(&b, &doc, &mut sy, &stem);
        eprintln!(
            "{stem}: synthetic tag probes: {} tags ({} probes, {} undecodable), queries {}/{} (both-error {}, pending IT-B {}, CCR-0014 {}, CCR-0015 {})",
            sy.tags, sy.probes, sy.undecodable_probes, sy.ok, sy.total, sy.errors_both, sy.pending_itb, sy.known_ccr0014, sy.known_ccr0015
        );
        for m in sy
            .mismatches
            .iter()
            .take(if verbose { usize::MAX } else { 5 })
        {
            eprintln!("  SYNTH MISMATCH {m}");
        }
        synth.tags = synth.tags.max(sy.tags);
        synth.probes = synth.probes.max(sy.probes);
        synth.undecodable_probes += sy.undecodable_probes;
        synth.known_undecodable_probes += sy.known_undecodable_probes;
        synth.total += sy.total;
        synth.ok += sy.ok;
        synth.errors_both += sy.errors_both;
        synth.pending_itb += sy.pending_itb;
        synth.known_ccr0014 += sy.known_ccr0014;
        synth.known_ccr0015 += sy.known_ccr0015;
        for (k, (n, ok)) in sy.by_tag {
            let e = synth.by_tag.entry(k).or_default();
            e.0 += n;
            e.1 += ok;
        }
        synth.mismatches.extend(sy.mismatches);
        eprintln!(
            "{stem}: {} stores, {} actors, {} items, {} cfgs, {} mods; queries {}/{} (both-error {}, known CCR-0014 {}, CCR-0015 {}, pending IT-B {}, undecodable {})",
            b.stores.len(),
            b.actors.len(),
            b.items.len(),
            b.cfgs.len(),
            b.mods.len(),
            t.ok,
            t.total,
            t.errors_both,
            t.known_ccr0014,
            t.known_ccr0015,
            t.pending_itb,
            t.undecodable
        );
        let shown = if verbose { t.mismatches.len() } else { 5 };
        for m in t.mismatches.iter().take(shown) {
            eprintln!("  MISMATCH {m}");
        }
        nbuilds += 1;
        nstores += b.stores.len();
        nactors += b.actors.len();
        nitems += b.items.len();
        ncfgs += b.cfgs.len();
        undecodable += b.undecodable;
        known_undecodable += b.known_undecodable;
        ge_copies += b.ge_copies;
        for (k, v) in &b.tag_types {
            *tag_types.entry(k.clone()).or_default() += v;
        }
        total.total += t.total;
        total.ok += t.ok;
        total.errors_both += t.errors_both;
        total.known_ccr0014 += t.known_ccr0014;
        total.known_ccr0015 += t.known_ccr0015;
        total.pending_itb += t.pending_itb;
        total.undecodable += t.undecodable;
        for (k, (n, ok)) in t.by_query {
            let e = total.by_query.entry(k).or_default();
            e.0 += n;
            e.1 += ok;
        }
        for (k, v) in t.error_kinds {
            *total.error_kinds.entry(k).or_default() += v;
        }
        total.mismatches.extend(t.mismatches);
    }
    eprintln!(
        "MS-B L3 full matrix over {nbuilds} builds: {nstores} stores, {nactors} actors, {nitems} items, {ncfgs} cfgs ({ge_copies} with a copied skillGrantedEffect); {undecodable} undecodable mods (+{known_undecodable} with CCR-0014 §3 tag keys)\n  \
         queries identical: {}/{} ({} of them PoB errors reproduced)\n  \
         known differences: CCR-0014 §2 (enemy activeSkillList) {}; CCR-0015 §3 (nil actor fields) {}\n  \
         pending IT-B (Item:FindModifierSubstring stub reached and result differs): {}\n  \
         skipped (a mod of the queried names is undecodable, CCR-0014 §3): {}",
        total.ok,
        total.total,
        total.errors_both,
        total.known_ccr0014,
        total.known_ccr0015,
        total.pending_itb,
        total.undecodable
    );
    eprintln!("  per query (identical/total):");
    for (k, (n, ok)) in &total.by_query {
        eprintln!("    {k:<20} {ok}/{n}");
    }
    eprintln!("  tag types of the dumped mods (EvalMod branch coverage by real data):");
    for (k, v) in &tag_types {
        eprintln!("    {v:>8}  {k}");
    }
    eprintln!("  errors raised by both (PoB <-> Rust):");
    for (k, v) in &total.error_kinds {
        eprintln!("    {v:>7}  {k}");
    }
    eprintln!(
        "MS-B synthetic tag probes ({} distinct tags of the parser cache and skill data, {} probe mods; undecodable: {}, +{} warcry tags with CCR-0014 §3 keys) on the player/enemy/minion modDB of every build:\n  \
         queries identical: {}/{} ({} PoB errors reproduced); pending IT-B {}; known differences CCR-0014 §2 {}, CCR-0015 §3 (nil actor fields) {}",
        synth.tags,
        synth.probes,
        synth.undecodable_probes,
        synth.known_undecodable_probes,
        synth.ok,
        synth.total,
        synth.errors_both,
        synth.pending_itb,
        synth.known_ccr0014,
        synth.known_ccr0015
    );
    eprintln!("  per tag type (identical/total):");
    for (k, (n, ok)) in &synth.by_tag {
        eprintln!("    {k:<20} {ok}/{n}");
    }
    assert_eq!(undecodable, 0, "mods the Rust model cannot decode");
    assert!(total.total > 0);
    assert_eq!(total.ok, total.total, "L3 queries differ");
    assert_eq!(
        synth.undecodable_probes, 0,
        "tags the Rust model cannot decode"
    );
    assert!(synth.total > 0);
    assert_eq!(synth.ok, synth.total, "synthetic tag probes differ");
    // coverage and the excused buckets (HYG2/V2). IT-B is merged, so `pending IT-B` must be 0;
    // the CCR-0014 / CCR-0015 known differences and the undecodable-name skips are bounded.
    assert_eq!(
        (total.pending_itb, synth.pending_itb),
        (0, 0),
        "pending IT-B (Item:FindModifierSubstring stub reached)"
    );
    let filtered =
        std::env::var_os("MS_B_BUILDS").is_some() || std::env::var_os("MS_B_LIMIT").is_some();
    if !filtered {
        assert_eq!(nbuilds, CORPUS_BUILDS, "builds compared");
        let b = MS_B_BOUNDS;
        assert!(
            total.total >= b.min_queries.0 && synth.total >= b.min_queries.1,
            "{} / {} queries compared, expected at least {:?}",
            total.total,
            synth.total,
            b.min_queries
        );
        let got = (
            total.known_ccr0014 + synth.known_ccr0014,
            total.known_ccr0015 + synth.known_ccr0015,
            total.undecodable,
            known_undecodable + synth.known_undecodable_probes,
        );
        assert!(
            got.0 <= b.max_ccr0014
                && got.1 <= b.max_ccr0015
                && got.2 <= b.max_undecodable_skips
                && got.3 <= b.max_known_undecodable,
            "buckets (CCR-0014, CCR-0015, undecodable-name skips, CCR-0014 §3 tag-key mods) {got:?}; bounds {b:?}"
        );
    }
}

/// The corpus size (`.parity/builds/*.xml`).
const CORPUS_BUILDS: usize = 57;

#[derive(Debug)]
struct MsBBounds {
    /// `(matrix queries, synthetic tag-probe queries)` compared.
    min_queries: (usize, usize),
    /// CCR-0014 §2 known differences (matrix + synthetic).
    max_ccr0014: usize,
    /// CCR-0015 §3 known differences (matrix + synthetic).
    max_ccr0015: usize,
    /// Matrix queries skipped because a mod of the queried names is undecodable.
    max_undecodable_skips: usize,
    /// Mods / probes with CCR-0014 §3 tag keys the Rust model does not decode.
    max_known_undecodable: usize,
}

/// Measured by HYG2 on rust-port 746b103.
const MS_B_BOUNDS: MsBBounds = MsBBounds {
    min_queries: (6_948_606, 5_668_313),
    max_ccr0014: 10_360,
    max_ccr0015: 1192,
    max_undecodable_skips: 638,
    max_known_undecodable: 9,
};

// ---------------------------------------------------------------------------------------------
// tag.div no-reader proof
// ---------------------------------------------------------------------------------------------

/// Installs, in a booted PoB, a wrapper around `ModStore:EvalMod` that restores every
/// `divVar` tag's raw `div` when the call returns (or raises): the Rust semantics, where the
/// value lives in a local. Counts the calls that restored something.
const RESTORE_DIV_LUA: &str = r#"
__msb_divRestores = 0
local orig = common.classes.ModStore.EvalMod
local function wrapped(self, mod, cfg, globalLimits)
	local saved
	for _, tag in ipairs(mod) do
		if type(tag) == "table" and tag.divVar then
			saved = saved or { }
			saved[#saved + 1] = { tag, rawget(tag, "div") }
		end
	end
	if not saved then
		return orig(self, mod, cfg, globalLimits)
	end
	local ok, r = pcall(orig, self, mod, cfg, globalLimits)
	for _, s in ipairs(saved) do
		rawset(s[1], "div", s[2])
	end
	__msb_divRestores = __msb_divRestores + 1
	if not ok then error(r, 0) end
	return r
end
for _, name in ipairs({ "ModStore", "ModDB", "ModList" }) do
	rawset(common.classes[name], "EvalMod", wrapped)
end
return "ok"
"#;

/// After the build has loaded: the outputs every calc reader produces (BUILD-mode env and the
/// CALCS-mode env, player/enemy/minion outputs), the divVar tags' current raw `div`, then a
/// second full recalculation (stale `div`s from the first run are visible to it in stock PoB)
/// and the same outputs again. Returns JSON.
const OBSERVE_LUA: &str = r#"
local fmt, concat, sort, floor, huge = string.format, table.concat, table.sort, math.floor, math.huge
local function jnum(v)
	if v ~= v then return '"nan"' end
	if v == huge then return '"inf"' end
	if v == -huge then return '"-inf"' end
	if v == floor(v) and v > -2^53 and v < 2^53 then return fmt("%d", v) end
	return fmt("%.17g", v)
end
local escapes = { ['"'] = '\\"', ['\\'] = '\\\\', ['\n'] = '\\n', ['\t'] = '\\t', ['\r'] = '\\r' }
local function jstr(s)
	return '"' .. s:gsub('[%c"\\]', function(c) return escapes[c] or fmt("\\u%04x", c:byte()) end) .. '"'
end
local enc
enc = function(v, depth, seen)
	local tv = type(v)
	if tv == "number" then return jnum(v)
	elseif tv == "string" then return jstr(v)
	elseif tv == "boolean" then return tostring(v)
	elseif tv ~= "table" then return '"<' .. tv .. '>"' end
	if seen[v] or depth > 8 or rawget(v, "Object") or v._className then return '"<ref>"' end
	seen[v] = true
	-- identity keys (tables) print as addresses, which differ between processes: count them
	local keys, nIdentity = { }, 0
	for k in pairs(v) do
		if type(k) == "string" or type(k) == "number" then keys[#keys + 1] = k else nIdentity = nIdentity + 1 end
	end
	sort(keys, function(a, b) return tostring(a) < tostring(b) end)
	local parts = { }
	if nIdentity > 0 then parts[1] = '"<identity keys>":' .. nIdentity end
	for _, k in ipairs(keys) do parts[#parts + 1] = jstr(tostring(k)) .. ":" .. enc(v[k], depth + 1, seen) end
	seen[v] = nil
	return "{" .. concat(parts, ",") .. "}"
end
local function snapshot()
	local parts = { }
	for _, which in ipairs({ "mainEnv", "calcsEnv" }) do
		local e = build.calcsTab[which]
		if e then
			for _, a in ipairs({ "player", "enemy", "minion" }) do
				if e[a] and e[a].output then
					parts[#parts + 1] = jstr(which .. "." .. a) .. ":" .. enc(e[a].output, 0, { })
				end
			end
		end
	end
	return "{" .. concat(parts, ",") .. "}"
end
local divs = { }
for _, sk in pairs(data.skills) do
	for stat, map in pairs(sk.statMap or { }) do
		for _, m in ipairs(map) do
			for _, tag in ipairs(m.name and m or { }) do
				if type(tag) == "table" and tag.divVar then
					divs[#divs + 1] = jstr(sk.id .. ":" .. stat .. ":" .. tag.divVar) .. ":" .. (rawget(tag, "div") and jnum(rawget(tag, "div")) or "null")
				end
			end
		end
	end
end
sort(divs)
local main = build.calcsTab.mainEnv.player.mainSkill
local first = snapshot()
build.calcsTab:BuildOutput()
local second = snapshot()
return "{" .. concat({
	'"mainSkill":' .. jstr(main and main.activeEffect.grantedEffect.id or "none"),
	'"disabled":' .. ((main and main.skillFlags.disable) and "true" or "false"),
	'"skillPart":' .. (main and main.skillPart and jnum(main.skillPart) or "null"),
	'"rageSacrificed":' .. jnum(main and main.skillModList:Sum("BASE", main.skillCfg, "Multiplier:RageSacrificed") or 0),
	'"divRestores":' .. tostring(__msb_divRestores or -1),
	'"divs":{' .. concat(divs, ",") .. "}",
	'"first":' .. first,
	'"second":' .. second,
}, ",") .. "}"
"#;

/// A Rage Vortex variant of a build: a new socket group with Rage Vortex (skill part 2, Rage
/// Storm, whose stat map holds the only two `divVar` tags in PoB's data) made the main group.
fn rage_vortex_variant(xml: &str) -> Option<String> {
    let gem = r#"<Skill enabled="true" includeInFullDPS="nil" label="MS-B divVar" mainActiveSkill="1" mainActiveSkillCalcs="1"><Gem enableGlobal1="true" enableGlobal2="true" enabled="true" gemId="Metadata/Items/Gems/SkillGemRageVortex" level="20" nameSpec="Rage Vortex" quality="20" skillId="RageVortex" variantId="RageVortex" skillPart="2" skillPartCalcs="2"/></Skill>"#;
    // the active skill set (or the bare <Skills> list)
    let set_start = xml.find("<SkillSet ").or_else(|| xml.find("<Skills"))?;
    let close = if xml[set_start..].starts_with("<SkillSet") {
        set_start + xml[set_start..].find("</SkillSet>")?
    } else {
        set_start + xml[set_start..].find("</Skills>")?
    };
    let n = xml[set_start..close].matches("<Skill ").count();
    let mut out = String::with_capacity(xml.len() + gem.len());
    out.push_str(&xml[..close]);
    out.push_str(gem);
    out.push_str(&xml[close..]);
    // mainSocketGroup="k" on <Build ...>
    let b = out.find("<Build ")?;
    let m = b + out[b..].find("mainSocketGroup=\"")? + "mainSocketGroup=\"".len();
    let e = m + out[m..].find('"')?;
    out.replace_range(m..e, &(n + 1).to_string());
    Some(out)
}

/// The paths where two JSON trees differ (at most `max`).
fn json_diff(a: &J, b: &J, path: &str, out: &mut Vec<String>, max: usize) {
    if out.len() >= max || a == b {
        return;
    }
    match (a, b) {
        (J::Object(x), J::Object(y)) => {
            let mut keys: Vec<&String> = x.keys().chain(y.keys()).collect();
            keys.sort();
            keys.dedup();
            for k in keys {
                json_diff(
                    x.get(k).unwrap_or(&J::Null),
                    y.get(k).unwrap_or(&J::Null),
                    &format!("{path}.{k}"),
                    out,
                    max,
                );
            }
        }
        _ => out.push(format!("{path}: {a} vs {b}")),
    }
}

fn assert_same(stock: &J, restored: &J, what: &str, name: &str) {
    let mut d = Vec::new();
    json_diff(stock, restored, what, &mut d, 30);
    assert!(
        d.is_empty(),
        "{name}: {what} differs (stock vs restored):\n  {}",
        d.join("\n  ")
    );
}

fn observe(src: &Path, xml: &str, name: &str, restore: bool) -> J {
    let _lock = OracleLock::acquire().expect("oracle lock");
    let mut pob = JitPob::boot_oracle(src, None).unwrap_or_else(|e| panic!("boot: {e}"));
    if restore {
        pob.run(RESTORE_DIV_LUA)
            .unwrap_or_else(|e| panic!("patch: {e}"));
    }
    pob.load_build_xml(xml, name)
        .unwrap_or_else(|e| panic!("{name}: load: {e}"));
    let out = match pob
        .run(OBSERVE_LUA)
        .unwrap_or_else(|e| panic!("{name}: observe: {e}"))
        .first()
    {
        Some(Ret::Str(s)) => s.clone(),
        other => panic!("{name}: observe returned {other:?}"),
    };
    serde_json::from_str(&out).unwrap_or_else(|e| panic!("{name}: json: {e}"))
}

/// calc doc §5.3 / §11.1 MS-B: "`tag.div` no-reader proof".
///
/// * Static: every `divVar` tag in PoB's data and parser is one of the two Rage Vortex stat-map
///   mods (act_str.lua:9102/9105), and the only reads of a tag's `div` key in PoB's source are
///   EvalMod's own (ModStore.lua 403/405/501, each preceded by the write in the same call),
///   CalcBreakdownControl.lua 481/485 (UI text; CCR-0015 §1) and the warcry `GlobalEffect` tag
///   read at CalcPerform.lua:2381-2382 (never a divVar tag). Whole-tag readers (copyTable,
///   modLib.formatTag) see `div` only on those two mods, which no MergeMod/compare path touches.
/// * Dynamic: PoB is run twice per build in fresh processes, stock and with EvalMod wrapped so
///   that the `div` write is undone when the call returns (the Rust semantics). All calc outputs
///   (BUILD and CALCS envs; after a second full recalculation too, which sees stale `div`s in
///   stock PoB) must be identical, on a Rage Vortex variant of corpus builds (main skill Rage
///   Vortex, part 2, so the divVar tags are evaluated: asserted through the wrapper's count and
///   through the stock run's mutated `div`) and on the unmodified builds.
#[test]
fn tag_div_no_reader() {
    let Some(src) = pob_testpaths::require(
        find_pob_src(),
        "PoB checkout (.PathOfBuilding/src) not found",
    ) else {
        return; // POB_ALLOW_SKIP=1
    };
    let Some(parity) = pob_testpaths::require(find_parity_dir(), ".parity not found") else {
        return; // POB_ALLOW_SKIP=1
    };
    // --- static census ---
    let mut div_var_sites = Vec::new();
    let mut div_reads = Vec::new();
    let mut stack = vec![src.clone()];
    while let Some(dir) = stack.pop() {
        for e in std::fs::read_dir(&dir).unwrap().filter_map(Result::ok) {
            let p = e.path();
            if p.is_dir() {
                stack.push(p);
                continue;
            }
            if p.extension().is_none_or(|x| x != "lua") {
                continue;
            }
            let rel = p.strip_prefix(&src).unwrap().to_string_lossy().into_owned();
            if rel.starts_with("Export/") {
                continue; // data export scripts (not shipped)
            }
            let text = std::fs::read(&p).unwrap();
            let text = String::from_utf8_lossy(&text);
            for (i, line) in text.lines().enumerate() {
                let code = line.split("--").next().unwrap_or("");
                if code.contains("divVar =") {
                    div_var_sites.push(format!("{rel}:{}", i + 1));
                }
                let mut rest = code;
                while let Some(pos) = rest.find(".div") {
                    let after = &rest[pos + 4..];
                    if !after.starts_with(|c: char| c.is_alphanumeric() || c == '_') {
                        div_reads.push(format!("{rel}:{}", i + 1));
                        break;
                    }
                    rest = after;
                }
            }
        }
    }
    div_var_sites.sort();
    div_reads.sort();
    div_reads.dedup();
    eprintln!("divVar tag sites: {div_var_sites:?}");
    eprintln!("`.div` reads/writes: {div_reads:?}");
    assert_eq!(
        div_var_sites,
        [
            "Data/Skills/act_str.lua:9102",
            "Data/Skills/act_str.lua:9105"
        ],
        "a new divVar tag site: re-check the no-reader proof"
    );
    assert_eq!(
        div_reads,
        [
            "Classes/CalcBreakdownControl.lua:481",
            "Classes/CalcBreakdownControl.lua:485",
            "Classes/ModStore.lua:401",
            "Classes/ModStore.lua:403",
            "Classes/ModStore.lua:405",
            "Classes/ModStore.lua:499",
            "Classes/ModStore.lua:501",
            "Modules/CalcActiveSkill.lua:69",
            "Modules/CalcActiveSkill.lua:72",
            "Modules/CalcPerform.lua:2381",
            "Modules/CalcPerform.lua:2382",
        ],
        "a new reader of a `div` key: re-check the no-reader proof"
    );

    // --- dynamic: stock vs restored, on Rage Vortex variants and plain builds ---
    let builds = corpus(&parity);
    if builds.is_empty() {
        let _ = pob_testpaths::require(None::<()>, ".parity/builds is empty");
        return; // POB_ALLOW_SKIP=1
    }
    let want_rv: usize = std::env::var("MS_B_DIV_RV")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(3);
    let want_plain: usize = std::env::var("MS_B_DIV_PLAIN")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(3);
    let mut rv_ok = 0;
    let mut plain_ok = 0;
    for xml in &builds {
        if rv_ok >= want_rv && plain_ok >= want_plain {
            break;
        }
        let stem = xml.file_stem().unwrap().to_string_lossy().into_owned();
        let text = std::fs::read_to_string(xml).unwrap();
        if rv_ok < want_rv {
            if let Some(v) = rage_vortex_variant(&text) {
                let name = format!("{stem}+RageVortex");
                let stock = observe(&src, &v, &name, false);
                if stock["mainSkill"] == "RageVortex" && stock["disabled"] == false {
                    let restored = observe(&src, &v, &name, true);
                    let restores = restored["divRestores"].as_i64().unwrap();
                    let stock_divs = stock["divs"].as_object().unwrap();
                    eprintln!(
                        "{name}: skillPart {} RageSacrificed {} — EvalMod calls on divVar tags {restores}; stock `div` after the calc {:?}; restored {:?}",
                        stock["skillPart"], stock["rageSacrificed"], stock_divs, restored["divs"]
                    );
                    assert!(restores > 0, "{name}: the divVar tags were never evaluated");
                    assert!(
                        stock_divs.values().any(|v| !v.is_null()),
                        "{name}: stock PoB did not write tag.div"
                    );
                    assert!(
                        restored["divs"]
                            .as_object()
                            .unwrap()
                            .values()
                            .all(J::is_null),
                        "{name}: the wrapper left a div behind"
                    );
                    assert_same(&stock["first"], &restored["first"], "first", &name);
                    assert_same(&stock["second"], &restored["second"], "second", &name);
                    rv_ok += 1;
                    continue;
                }
                eprintln!(
                    "{name}: main skill {} disabled {} (weapon cannot use Rage Vortex); next build",
                    stock["mainSkill"], stock["disabled"]
                );
            }
        }
        if plain_ok < want_plain {
            let stock = observe(&src, &text, &stem, false);
            let restored = observe(&src, &text, &stem, true);
            assert_same(&stock["first"], &restored["first"], "first", &stem);
            assert_same(&stock["second"], &restored["second"], "second", &stem);
            eprintln!(
                "{stem}: identical (EvalMod calls on divVar tags: {})",
                restored["divRestores"]
            );
            plain_ok += 1;
        }
    }
    eprintln!("tag.div no-reader: {rv_ok} Rage Vortex variants, {plain_ok} plain builds identical");
    assert!(rv_ok > 0, "no build could run Rage Vortex");
}
