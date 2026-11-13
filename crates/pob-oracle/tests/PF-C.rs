//! PF-C acceptance (calc doc §9.4 L4, unit table row PF-C): inject the main env's
//! `perform:1289` dump, run the Rust segment (`calc_perform::setup_flasks::seg`,
//! CalcPerform.lua 1289-1942) and diff the state against PoB's `perform:1943` dump.
//!
//! Callees that are not ported yet are replayed (calc doc §9.2 "Replay"):
//! `createMinionSkills` (AS), `initMinionModDB` (PF-B) and `applyEnemyModifiers`
//! (PF-A) through their recorded `:in → :out` deltas (`pob_oracle::replay`), keyed
//! by the hits PoB made directly from the same `calcs.perform` call; `mergeBuff`
//! (PF-A), whose arguments are locals no dump reaches, through a stand-in
//! transcription of CalcPerform.lua 45-67 below. A callee whose unit has landed
//! runs for real (the segment only hands a call to the hook when it hit the stub;
//! `setup_flasks::callee_replay`, CCR-0079 §2).
//!
//! Test-environment gaps, each worked around here and recorded in CCR-0079:
//! * §3: the pob-state/1 item dump has no `buffModList`; the flasks' lists are
//!   restored from the `pfc:calcFlaskMods` line dumps (their `buffModList` local).
//! * PFC1: `env.flasks` is keyed by item tables, so PoB's `pairs(flasks)` order (CalcPerform.lua
//!   1773) follows addresses and differs between PoB processes, and the dump sorts `$keys` by
//!   content; the order the run's `pfc:calcFlaskMods` calls show is restored before the segment
//!   runs ([`restore_flask_order`]). It picks the flask whose buff `mergeBuff` keeps.
//! * §4: the injected graph has no passive tree (PT, BM-A), so `modLib.mergeKeystones`
//!   (ported, CT) cannot resolve keystones; its recorded delta is replayed after it.
//! * `env.partyMembers` is the build model's `partyTab.actor` (CalcPerform.lua:1310);
//!   the model is empty until BM-A (`Build:LoadDB` is a stub), so its contents are
//!   not compared ([`MODEL_ONLY`]) and the aliasing is checked instead.
//! * Skills `createMinionSkills` (AS, real since AS landed) creates in the run: their typed
//!   `bool` fields that PoB's fresh tables leave nil are marked absent for the exporter
//!   ([`mark_fresh_skill_defaults`]); their buff mod lists' order inside one source's stat
//!   map is PoB's per-process `pairs` order and counts as LUA-ORDER
//!   (`testkit::buff_list_order_only`, via `testkit::diff_against_normalised`).
//!
//! The closures' upvalues (CCR-0079 §1): after the run, `PerformCtx.mergeFlasks` /
//! `.mergeTinctures` must hold the values PoB's locals had where it created the closures (the
//! line probes `perform:1691` / `perform:1862`; a Lua nil boolean is `false`). A difference fails
//! the build like a state mismatch.
//!
//! Dumps: the test probes every `.parity/builds` build itself with `corpus/PF-C/stages.toml`
//! and the selection [`SELECT`], through the shared content-keyed probe cache
//! (`testkit::probe_selection`, as PF-G does; a fresh `pob_oracle probe` process per build on a
//! miss), and reads only the main env's dumps ([`main_env_files`]). FS1: the dumps used to come
//! only from a manual `corpus/PF-C/probe.sh` run into `target/tmp/pfc-probe/`, so the full suite
//! (fresh target dir, `POB_ORACLE_REQUIRE=1`) failed with "no PF-C dumps" and every other run
//! skipped silently. `$PFC_PROBE_DIR` still reads a `probe.sh` output directory instead
//! (`<stem>/hits.json`, `<stem>/state/`). Missing inputs follow the workspace policy
//! (`pob_testpaths::require`: a failure unless `POB_ALLOW_SKIP=1`). `PFC_ONLY=<substring>`
//! restricts the builds.
#![allow(non_snake_case)]

use std::cell::RefCell;
use std::collections::{BTreeMap, VecDeque};
use std::path::{Path, PathBuf};
use std::rc::Rc;

use pob_calc::modules::calc_perform::setup_flasks::callee_replay;
use pob_calc::prelude::*;
use pob_oracle::diff::{Mismatch, MismatchKind};
use pob_oracle::replay::Replay;
use pob_oracle::schema::StateDump;
use pob_oracle::testkit;
use serde_json::Value as J;

/// The callees replayed from their recorded deltas. `mergeKeystones` is ported (CT) but
/// needs the passive tree (CCR-0079 §4); the segment reports every call of it.
const REPLAYED: &[&str] = &[
    "createMinionSkills",
    "initMinionModDB",
    "applyEnemyModifiers",
    "mergeKeystones",
];

/// Paths whose contents come from the (unloaded) build model, not from the segment.
const MODEL_ONLY: &[&str] = &["env.partyMembers."];

/// The probe selection (the same as `corpus/PF-C/probe.sh`): the segment's line probes and the
/// closure-upvalue probes of the main env, the `pfc:calcFlaskMods` line dumps and every call of
/// the replayed callees. The callee globs have no `@main` (that picks only each id's first main-env
/// hit), so they dump every env's calls; [`main_env_files`] keeps the main env's.
const SELECT: &[&str] = &[
    "perform:1289@main",
    "perform:1943@main",
    "perform:1691@main",
    "perform:1862@main",
    "pfc:calcFlaskMods*",
    "mergeKeystones:*",
    "createMinionSkills:*",
    "initMinionModDB:*",
    "applyEnemyModifiers:*",
];

fn stages_file() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("corpus/PF-C/stages.toml")
}

/// One build's input: a `$PFC_PROBE_DIR/<stem>/state` directory, or the build XML to probe.
enum Input {
    Dir(PathBuf),
    Xml(PathBuf),
}

/// Where one build's dumps are read from: the directory, or the shared probe cache's entry
/// (held while the dumps are read: a private `POB_QUERY_CACHE=off` run is deleted when dropped).
enum DumpSource {
    Dir(PathBuf),
    Probe(pob_oracle::runner::ProbeDumps),
}

impl DumpSource {
    fn state(&self) -> &Path {
        match self {
            DumpSource::Dir(p) => p,
            DumpSource::Probe(d) => d.path(),
        }
    }
}

impl Input {
    /// The build's dumps; an XML is probed now when the cache has no entry for it (one build at
    /// a time, so the test streams through the corpus).
    fn open(&self) -> Result<DumpSource, String> {
        match self {
            Input::Dir(d) => Ok(DumpSource::Dir(d.clone())),
            Input::Xml(xml) => {
                let opts = pob_oracle::runner::ProbeOptions {
                    select: SELECT.iter().map(|s| (*s).to_owned()).collect(),
                    max_dumps: 2000,
                    provenance: false,
                };
                testkit::probe_selection(
                    Path::new(env!("CARGO_BIN_EXE_pob_oracle")),
                    xml,
                    Some(&stages_file()),
                    &opts,
                )
                .map(DumpSource::Probe)
            }
        }
    }
}

/// The build inputs, `(stem, input)` in name order (`PFC_ONLY` filters by stem). `None`: an
/// input is missing and `POB_ALLOW_SKIP=1` skips.
fn build_inputs() -> Option<Vec<(String, Input)>> {
    let keep = |stem: &str| std::env::var("PFC_ONLY").map_or(true, |only| stem.contains(&only));
    let mut out = Vec::new();
    if let Some(root) = std::env::var_os("PFC_PROBE_DIR").map(PathBuf::from) {
        let mut dirs: Vec<PathBuf> = std::fs::read_dir(&root)
            .map(|rd| {
                rd.filter_map(Result::ok)
                    .map(|e| e.path())
                    .filter(|p| p.join("hits.json").is_file())
                    .collect()
            })
            .unwrap_or_default();
        dirs.sort();
        for d in dirs {
            let stem = d.file_name().unwrap().to_string_lossy().into_owned();
            if keep(&stem) {
                out.push((stem, Input::Dir(d.join("state"))));
            }
        }
        let what = format!("PF-C dumps under $PFC_PROBE_DIR={}", root.display());
        return pob_testpaths::require((!out.is_empty()).then_some(out), &what);
    }
    let parity = pob_testpaths::require(pob_testpaths::parity_dir(), ".parity")?;
    let mut xmls: Vec<PathBuf> = std::fs::read_dir(parity.join("builds"))
        .map(|rd| {
            rd.filter_map(Result::ok)
                .map(|e| e.path())
                .filter(|p| p.extension().is_some_and(|x| x == "xml"))
                .collect()
        })
        .unwrap_or_default();
    xmls.sort();
    for xml in xmls {
        let stem = xml
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or_default()
            .to_owned();
        if keep(&stem) {
            out.push((stem, Input::Xml(xml)));
        }
    }
    let what = format!("builds under {}", parity.join("builds").display());
    pob_testpaths::require((!out.is_empty()).then_some(out), &what)
}

/// The dump file names (without a `.zst` suffix) of the main env's hits, from the run's
/// `hits.json` (`hits.json.zst` in the probe cache) next to `state/`. `None` when the index
/// names no main env: every file is read then.
fn main_env_files(state: &Path) -> Result<Option<std::collections::BTreeSet<String>>, String> {
    let run = state.parent().ok_or("state dir without a parent")?;
    let hits = ["hits.json", "hits.json.zst"]
        .iter()
        .map(|f| run.join(f))
        .find(|p| p.is_file())
        .ok_or_else(|| format!("{}: no hits.json", run.display()))?;
    let idx = pob_oracle::schema::HitsIndex::load(&hits).map_err(|e| e.0)?;
    let Some(main) = idx.main_env else {
        return Ok(None);
    };
    Ok(Some(
        idx.dumps
            .iter()
            .filter(|d| d.env.as_deref() == Some(main.as_str()))
            .filter_map(|d| d.file.rsplit('/').next().map(str::to_owned))
            .collect(),
    ))
}

/// `mergeBuff(src, destTable, destKey)` (CalcPerform.lua 45-67), the stand-in until PF-A
/// lands: `destTable[destKey]` is an array table of mods (the segment reads either that or a
/// ModList object).
fn merge_buff_standin(src: &Table, destTable: &Table, destKey: Name) {
    // L46
    if !destTable.truthy(destKey) {
        // L47
        destTable.set(destKey, Table::new());
    }
    // L49
    let dest = destTable
        .table(destKey)
        .expect("destTable[destKey] is a table");
    // L50
    for (_, m) in src.ipairs() {
        let Val::Mod(m) = m else {
            panic!("mergeBuff src entry is not a mod")
        };
        // L51
        let mut matched = false;
        // L52
        if m.r#type != ModType::LIST {
            // L53
            for (index, destMod) in dest.ipairs() {
                let Val::Mod(destMod) = destMod else {
                    panic!("mergeBuff dest entry is not a mod")
                };
                // L54
                if pob_gamedata::mod_tools::compareModParams(&m, &destMod) {
                    // L55
                    if let Value::Num(dv) = destMod.value {
                        if Val::from_value(&m.value).num() > dv {
                            // L56
                            dest.set(index as i64, Val::Mod(m.clone()));
                        }
                    }
                    // L58
                    matched = true;
                    // L59
                    break;
                }
            }
        }
        // L63
        if !matched {
            // L64
            dest.push(Val::Mod(m));
        }
    }
}

/// One build's dumps: the segment's in/out and the callees' recorded calls.
struct BuildDumps {
    din: StateDump,
    dout: StateDump,
    /// the main env's `perform:1691` / `perform:1862` line dumps (CCR-0079 §1), when probed
    up1691: Option<StateDump>,
    up1862: Option<StateDump>,
    /// callee → its recorded calls and the hits the segment's perform made, in call order.
    replays: Rc<BTreeMap<&'static str, (Replay, Vec<String>)>>,
    /// item id → `item.buffModList` as PoB had it (from the `pfc:calcFlaskMods` dumps).
    buffModLists: BTreeMap<i64, Vec<Mod>>,
    /// The item ids of `env.flasks` in the order PoB's `pairs(flasks)` (CalcPerform.lua:1773)
    /// visited them in this process (the `pfc:calcFlaskMods` call order; see [`buff_mod_lists`]).
    flaskOrder: Vec<i64>,
}

/// The definition of `j` when it is a `{"$ref": n}` in `root`.
fn deref<'a>(root: &'a J, j: &'a J) -> &'a J {
    fn find(v: &J, id: u64) -> Option<&J> {
        match v {
            J::Object(m) => {
                if m.get("$id").and_then(J::as_u64) == Some(id) {
                    return Some(v);
                }
                m.values().find_map(|x| find(x, id))
            }
            J::Array(a) => a.iter().find_map(|x| find(x, id)),
            _ => None,
        }
    }
    match j.get("$ref").and_then(J::as_u64) {
        Some(id) if j.as_object().is_some_and(|m| m.len() == 1) => find(root, id).unwrap_or(j),
        _ => j,
    }
}

/// `item.buffModList` per item id from the `pfc:calcFlaskMods` line dumps of the segment's
/// `calcs.perform` call (CCR-0079 §3: the pob-state/1 item dump leaves the field out), and the
/// item ids in the order of those calls.
///
/// PFC1: the order is the one PoB's `for item in pairs(flasks)` (CalcPerform.lua:1773, from
/// `mergeFlasks(env.flasks, false, true)` at 1937) took in this process. `env.flasks` is keyed by
/// item tables, so that order follows table addresses and changes from one PoB process to the
/// next (allflame_exp_000, three fresh probes: items 23,20,19,10,15 / 15,10,19,20,23 /
/// 23,19,15,10,20). The state dump cannot carry it: `$keys` entries are sorted by content
/// (state_dump.lua `keyLabel`, Items by id), so the injected `env.flasks` is in id order. It
/// decides which of two flasks granting the same buff `mergeBuff` keeps (the first one merged:
/// CalcPerform.lua:54-59), e.g. the `source` of 000's `BleedImmune` (Item:20 or Item:23).
/// Only the calls with `baseName == item.baseName` and no `onlyMinion` are kept: those are the
/// 1937 call's (the 3121 call, after the segment, passes "LifeFlask"/"ManaFlask" or
/// `onlyMinion = true`); each flask of the loop makes exactly one.
fn buff_mod_lists(
    dumps: &BTreeMap<String, StateDump>,
    din: &StateDump,
) -> Result<(BTreeMap<i64, Vec<Mod>>, Vec<i64>), String> {
    let mut out = BTreeMap::new();
    let mut order: Vec<(u64, i64)> = Vec::new();
    for d in dumps.values() {
        if d.probe != "pfc:calcFlaskMods" || d.stack != din.stack || d.env_id != din.env_id {
            continue;
        }
        let root = &d.root;
        let locals = &root["locals"];
        if locals.get("onlyMinion").and_then(J::as_bool) == Some(true) {
            continue;
        }
        let item = deref(root, &locals["item"]);
        // recovery calls pass calcFlaskRecovery's list under "LifeFlask"/"ManaFlask"
        if locals["baseName"] != item["baseName"] {
            continue;
        }
        let list = deref(root, &locals["buffModList"]);
        let Some(id) = item.get("id").and_then(J::as_f64) else {
            return Err(format!("{}: item without id", d.hit));
        };
        let elems = match list {
            J::Array(a) => a.clone(),
            J::Object(m) => match m.get("$seq") {
                Some(J::Array(a)) => a.clone(),
                _ if m.keys().all(|k| k.starts_with('$')) => Vec::new(),
                _ => return Err(format!("{}: buffModList is not a list", d.hit)),
            },
            _ => return Err(format!("{}: buffModList is not a list", d.hit)),
        };
        let mut mods = Vec::new();
        for e in &elems {
            let e = deref(root, e);
            let (m, _) =
                pob_oracle::mod_json::decode_mod(e).map_err(|x| format!("{}: {x}", d.hit))?;
            mods.push(m);
        }
        out.insert(id as i64, mods);
        order.push((d.n, id as i64));
    }
    order.sort_unstable();
    Ok((out, order.into_iter().map(|(_, id)| id).collect()))
}

fn load_build_dumps(state: &Path) -> Result<BuildDumps, String> {
    let keep = main_env_files(state)?;
    let mut dumps = BTreeMap::new();
    for e in std::fs::read_dir(state).map_err(|e| format!("{}: {e}", state.display()))? {
        let p = e.map_err(|e| e.to_string())?.path();
        let name = p.file_name().and_then(|n| n.to_str()).unwrap_or_default();
        let name = name.strip_suffix(".zst").unwrap_or(name);
        let is_json = Path::new(name).extension().is_some_and(|x| x == "json");
        if !is_json || keep.as_ref().is_some_and(|k| !k.contains(name)) {
            continue;
        }
        let d = StateDump::load(&p).map_err(|e| format!("{}: {e}", p.display()))?;
        dumps.insert(d.hit.clone(), d);
    }
    let first = |probe: &str| {
        dumps
            .values()
            .filter(|d| d.probe == probe)
            .min_by_key(|d| d.n)
            .cloned()
            .ok_or_else(|| format!("no {probe} dump"))
    };
    let din = first("perform:1289")?;
    let dout = first("perform:1943")?;
    let up1691 = first("perform:1691").ok();
    let up1862 = first("perform:1862").ok();
    let mut replays = BTreeMap::new();
    for callee in REPLAYED {
        let r = Replay::load(callee, &dumps);
        // the calls the segment's `calcs.perform` made itself, in call order
        let mut hits: Vec<(u64, String)> = r
            .calls
            .iter()
            .filter(|(_, c)| c.din.stack == din.stack && c.din.env_id == din.env_id)
            .map(|(h, c)| (c.din.n, h.clone()))
            .collect();
        hits.sort();
        replays.insert(*callee, (r, hits.into_iter().map(|(_, h)| h).collect()));
    }
    let (buffModLists, flaskOrder) = buff_mod_lists(&dumps, &din)?;
    Ok(BuildDumps {
        din,
        dout,
        up1691,
        up1862,
        replays: Rc::new(replays),
        buffModLists,
        flaskOrder,
    })
}

/// What one build's L4 run produced.
#[derive(Default)]
struct Outcome {
    mismatches: Vec<Mismatch>,
    order_only: usize,
    model_only: usize,
    replayed: usize,
    leftover: usize,
    replay_notes: Vec<String>,
    party_alias_ok: bool,
    /// closure upvalues that differ from PoB's `perform:1691` / `perform:1862` locals
    upvalue_mismatches: Vec<String>,
    /// closures whose upvalues were compared (0, 1 or 2)
    upvalues_checked: usize,
    error: Option<String>,
}

/// CCR-0079 §1: the ctx's closure upvalues against the line dump PoB made where it created the
/// closure. `fields` are (Lua local name, the Rust value as a Lua value).
fn check_upvalues(probe: &str, dump: &StateDump, fields: &[(&str, Val)], out: &mut Vec<String>) {
    let locals = dump.locals().and_then(J::as_object);
    for (name, ours) in fields {
        let pob = locals
            .and_then(|l| l.get(*name))
            .cloned()
            .unwrap_or(J::Null);
        let same = match (ours, &pob) {
            (Val::Num(a), J::Number(b)) => b.as_f64() == Some(*a),
            (Val::Num(a), J::Object(_)) => pob_oracle::schema::decode_num(&pob) == Some(*a),
            (Val::Bool(a), J::Bool(b)) => a == b,
            // `env.minion and modDB:Flag(…)` is nil without a minion
            (Val::Bool(false), J::Null) => true,
            _ => false,
        };
        if !same {
            out.push(format!("{probe} {name}: pob={pob} ours={ours:?}"));
        }
    }
}

/// Set `item.buffModList` of the env's flasks from PoB's lists (by item id).
fn restore_buff_mod_lists(env: &EnvRef, lists: &BTreeMap<i64, Vec<Mod>>) {
    for (item, _) in env.flasks.get().with(|f| f.pairs()) {
        let Some(id) = item.id.get() else { continue };
        if let Some(mods) = lists.get(&(id as i64)) {
            let l = ModStore::ModList(None);
            for m in mods {
                l.AddMod(std::sync::Arc::new(m.clone()));
            }
            item.buffModList.set(Some(l));
        }
    }
}

/// PFC1: put `env.flasks` in the order PoB's `pairs` visited it (`order`, item ids; see
/// [`buff_mod_lists`]); the port's `pairs` is insertion order (setup_flasks.rs L1773
/// LUA-ORDER). Flasks PoB did not visit keep their relative order after the visited ones.
fn restore_flask_order(env: &EnvRef, order: &[i64]) {
    if order.is_empty() {
        return;
    }
    env.flasks.get().with_mut(|f| {
        let mut entries = f.pairs();
        let rank = |item: &ItemRef| {
            item.id
                .get()
                .and_then(|id| order.iter().position(|o| *o as f64 == id))
                .unwrap_or(order.len())
        };
        entries.sort_by_key(|(item, _)| rank(item));
        f.wipe();
        for (item, v) in entries {
            f.set(&item, Some(v));
        }
    });
}

/// CalcPerform.lua 1310-1311: `env.partyMembers` and `env.player.partyMembers` are the build's
/// `partyTab.actor`.
fn party_alias_ok(env: &EnvRef) -> bool {
    let party = &env.build.partyTab.actor;
    env.partyMembers
        .get()
        .is_some_and(|p| Rc::ptr_eq(&p, party))
        && env
            .player
            .partyMembers
            .get()
            .upgrade()
            .is_some_and(|p| Rc::ptr_eq(&p, party))
}

/// Every active skill reachable from the env's actors and `env.auxSkillList` (minions'
/// skill lists and main skills recursively), each once.
fn reachable_skills(env: &EnvRef) -> Vec<ActiveSkillRef> {
    use pob_calc::probe::rc_ptr;
    fn actor(
        a: &ActorRef,
        seen: &mut Vec<usize>,
        out: &mut Vec<ActiveSkillRef>,
        actors: &mut Vec<usize>,
    ) {
        if actors.contains(&rc_ptr(a)) {
            return;
        }
        actors.push(rc_ptr(a));
        let list = a.activeSkillList.get().with(Clone::clone);
        for s in list.iter().chain(a.mainSkill.get().iter()) {
            skill(s, seen, out, actors);
        }
        if let Some(m) = a.minion.get() {
            actor(&m, seen, out, actors);
        }
    }
    fn skill(
        s: &ActiveSkillRef,
        seen: &mut Vec<usize>,
        out: &mut Vec<ActiveSkillRef>,
        actors: &mut Vec<usize>,
    ) {
        if seen.contains(&rc_ptr(s)) {
            return;
        }
        seen.push(rc_ptr(s));
        out.push(s.clone());
        if let Some(m) = s.minion.get() {
            actor(&m, seen, out, actors);
        }
    }
    let (mut seen, mut out, mut actors) = (Vec::new(), Vec::new(), Vec::new());
    actor(&env.player, &mut seen, &mut out, &mut actors);
    if let Some(m) = env.minion.get() {
        actor(&m, &mut seen, &mut out, &mut actors);
    }
    for s in env.auxSkillList.get().with(Clone::clone) {
        skill(&s, &mut seen, &mut out, &mut actors);
    }
    out
}

/// Harness bookkeeping for the skills the port created during the run (createMinionSkills,
/// AS, runs for real since AS landed): the frozen `ActiveSkill` / `SkillEffect` hold
/// `buffSkill` … `minionBuffSkill` and `superseded` / `appliesToGrantedSkills` / `fromItem` as
/// typed `bool`s, `false` until set, where PoB's fresh tables leave them nil
/// (`createActiveSkill`'s table, CalcActiveSkill.lua:85-93; the minion skill's
/// `activeEffect = { grantedEffect, level, quality }`, CalcActiveSkill.lua:900-904). Lua only
/// ever assigns `true` to the four skill flags (CalcPerform.lua:2323-2958) and every reader
/// tests truthiness, so `false` is nil for the calc; the exporter only drops a default-valued
/// typed field that is marked absent, and the injection marks only the objects it built.
/// Mark the still-`false` fields of the skills (and their active effects) that were not in the
/// injected graph (`before`, kept alive so no address is reused) absent.
fn mark_fresh_skill_defaults(env: &EnvRef, before: &[ActiveSkillRef]) {
    use pob_calc::probe::{mark_absent, rc_ptr};
    let old_skills: Vec<usize> = before.iter().map(rc_ptr).collect();
    let old_effects: Vec<usize> = before
        .iter()
        .map(|s| rc_ptr(&s.activeEffect.get()))
        .collect();
    for s in reachable_skills(env) {
        let p = rc_ptr(&s);
        if old_skills.contains(&p) {
            continue;
        }
        for (k, v) in [
            ("buffSkill", s.buffSkill.get()),
            ("debuffSkill", s.debuffSkill.get()),
            ("totemBuffSkill", s.totemBuffSkill.get()),
            ("minionBuffSkill", s.minionBuffSkill.get()),
        ] {
            if !v {
                mark_absent(p, k);
            }
        }
        let ae = s.activeEffect.get();
        let pe = rc_ptr(&ae);
        if old_effects.contains(&pe) {
            continue;
        }
        for (k, v) in [
            ("superseded", ae.superseded.get()),
            ("appliesToGrantedSkills", ae.appliesToGrantedSkills.get()),
            ("fromItem", ae.fromItem.get()),
        ] {
            if !v {
                mark_absent(pe, k);
            }
        }
    }
}

/// Non-vacuity: the mismatches of the *unrun* `perform:1289` state against `perform:1943`
/// (the segment's own effect, which the run must account for).
fn unrun_mismatches(stem: &str, b: &BuildDumps) -> usize {
    let model = testkit::empty_build();
    let Ok(inj) = pob_oracle::inject::inject(&b.din, &model) else {
        return 0;
    };
    let locals: Vec<(String, Val)> = inj.locals.into_iter().collect();
    testkit::diff_against(&b.dout, inj.env.as_ref(), &locals, stem)
        .into_iter()
        .filter(|m| {
            m.kind != MismatchKind::LuaOrder && !MODEL_ONLY.iter().any(|p| m.path.starts_with(p))
        })
        .count()
}

fn run_build(stem: &str, b: &BuildDumps) -> Outcome {
    let mut out = Outcome::default();
    let model = testkit::empty_build();
    let inj = match pob_oracle::inject::inject(&b.din, &model) {
        Ok(i) => i,
        Err(e) => {
            out.error = Some(format!("inject: {e}"));
            return out;
        }
    };
    let Some(env) = inj.env.clone() else {
        out.error = Some("no env in perform:1289".to_owned());
        return out;
    };
    let locals: Vec<(String, Val)> = inj.locals.into_iter().collect();
    // CCR-0079 §3: restore the flasks' buffModList the item dump leaves out (again after
    // every replay: `apply_onto` refills the items from the export, which lacks the field)
    let lists = Rc::new(b.buffModLists.clone());
    restore_buff_mod_lists(&env, &lists);
    // PFC1: and PoB's per-process `pairs` order of `env.flasks`, which the dump sorts away
    restore_flask_order(&env, &b.flaskOrder);
    let injected_skills = reachable_skills(&env);

    // the replay hook
    let queues: Rc<RefCell<BTreeMap<&'static str, VecDeque<String>>>> = Rc::new(RefCell::new(
        b.replays
            .iter()
            .map(|(k, (_, hits))| (*k, hits.iter().cloned().collect()))
            .collect(),
    ));
    let log: Rc<RefCell<(usize, Vec<String>)>> = Rc::default();
    {
        let queues = queues.clone();
        let log = log.clone();
        let env = env.clone();
        let replays = b.replays.clone();
        let lists = lists.clone();
        let flaskOrder = b.flaskOrder.clone();
        callee_replay::set_hook(Some(Box::new(move |name, args| {
            if name == "mergeBuff" {
                let (Some(src), Some(dest), Some(key)) = (
                    args.first().and_then(Val::table),
                    args.get(1).and_then(Val::table),
                    args.get(2).and_then(Val::str),
                ) else {
                    log.borrow_mut()
                        .1
                        .push("mergeBuff: bad arguments".to_owned());
                    return;
                };
                merge_buff_standin(&src, &dest, Name::from(&*key));
                return;
            }
            let hit = queues
                .borrow_mut()
                .get_mut(name)
                .and_then(VecDeque::pop_front);
            let Some(hit) = hit else {
                log.borrow_mut()
                    .1
                    .push(format!("{name}: no recorded call left"));
                return;
            };
            let r = replays[name].0.apply_report(&hit, &env);
            restore_buff_mod_lists(&env, &lists);
            restore_flask_order(&env, &flaskOrder);
            match r {
                Ok(a) => {
                    let mut l = log.borrow_mut();
                    l.0 += 1;
                    for n in a.notes {
                        l.1.push(format!("{hit}: {n}"));
                    }
                }
                Err(e) => log.borrow_mut().1.push(format!("{hit}: {e}")),
            }
        })));
    }
    let run = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        pob_calc::probe::run_segment("perform:1289", &locals)
    }));
    callee_replay::set_hook(None);
    let (replayed, notes) = std::mem::take(&mut *log.borrow_mut());
    out.replayed = replayed;
    out.replay_notes = notes;
    let after = match run {
        Ok(Ok((_flow, after))) => after,
        Ok(Err(e)) => {
            out.error = Some(format!("run: {e}"));
            return out;
        }
        Err(p) => {
            let msg = p
                .downcast_ref::<pob_lua::LuaError>()
                .map(|e| e.0.clone())
                .or_else(|| p.downcast_ref::<String>().cloned())
                .or_else(|| p.downcast_ref::<&str>().map(|s| (*s).to_owned()))
                .unwrap_or_else(|| "panic".to_owned());
            out.error = Some(format!("run: {msg}"));
            return out;
        }
    };
    // recorded calls left over are the ones the rest of `calcs.perform` makes after 1943
    // (applyEnemyModifiers in doActorMisc, mergeKeystones in later segments)
    out.leftover = queues.borrow().values().map(VecDeque::len).sum();
    let local = |k: &str| after.iter().find(|(n, _)| n == k).map(|(_, v)| v.clone());
    if let (Some(d), Some(up)) = (
        &b.up1691,
        local("mergeFlasks")
            .and_then(|v| v.obj::<pob_calc::modules::calc_perform::setup_flasks::FlaskUpvalues>()),
    ) {
        out.upvalues_checked += 1;
        check_upvalues(
            "perform:1691",
            d,
            &[
                ("effectInc", Val::Num(up.effectInc)),
                ("effectIncMagic", Val::Num(up.effectIncMagic)),
                (
                    "effectIncMagicNoAdjacent",
                    Val::Num(up.effectIncMagicNoAdjacent),
                ),
                ("effectIncNonPlayer", Val::Num(up.effectIncNonPlayer)),
                (
                    "effectIncMagicNonPlayer",
                    Val::Num(up.effectIncMagicNonPlayer),
                ),
                ("flasksApplyToMinion", Val::Bool(up.flasksApplyToMinion)),
                (
                    "quickSilverAppliesToAllies",
                    Val::Bool(up.quickSilverAppliesToAllies),
                ),
                ("flaskTotalRateInc", Val::Num(up.flaskTotalRateInc)),
                ("flaskDurInc", Val::Num(up.flaskDurInc)),
            ],
            &mut out.upvalue_mismatches,
        );
    }
    if let (Some(d), Some(up)) = (
        &b.up1862,
        local("mergeTinctures").and_then(|v| {
            v.obj::<pob_calc::modules::calc_perform::setup_flasks::TinctureUpvalues>()
        }),
    ) {
        out.upvalues_checked += 1;
        check_upvalues(
            "perform:1862",
            d,
            &[
                ("effectInc", Val::Num(up.effectInc)),
                ("effectIncMagic", Val::Num(up.effectIncMagic)),
                ("tinctureLimit", Val::Num(up.tinctureLimit.get())),
            ],
            &mut out.upvalue_mismatches,
        );
    }
    if b.up1691.is_some() != (local("mergeFlasks").is_some_and(|v| !v.is_nil()))
        || b.up1862.is_some() != (local("mergeTinctures").is_some_and(|v| !v.is_nil()))
    {
        out.upvalue_mismatches.push(
            "a closure PoB created (perform:1691 / 1862 hit) is not in the ctx, or the reverse"
                .to_owned(),
        );
    }
    out.party_alias_ok = party_alias_ok(&env);
    mark_fresh_skill_defaults(&env, &injected_skills);
    // FUP1: the buff mod lists' LUA-ORDER check is the shared testkit normalisation (one
    // `LuaOrder` mismatch per order-only `…buffList[i].modList`)
    let mm = testkit::diff_against_normalised(&b.dout, Some(&env), &after, stem);
    for m in mm {
        if m.kind == MismatchKind::LuaOrder {
            out.order_only += 1;
        } else if MODEL_ONLY.iter().any(|p| m.path.starts_with(p)) {
            out.model_only += 1;
        } else {
            out.mismatches.push(m);
        }
    }
    out
}

/// `env.player.output.Foo` / `…mods.Life[3]` → a path class for the summary.
fn generalize(path: &str) -> String {
    let mut s = String::new();
    let mut in_idx = false;
    for c in path.chars() {
        match c {
            '[' => {
                in_idx = true;
                s.push_str("[]");
            }
            ']' => in_idx = false,
            _ if in_idx => {}
            _ => s.push(c),
        }
    }
    s
}

fn clip(s: &str) -> String {
    if s.len() > 160 {
        let mut e = 160;
        while !s.is_char_boundary(e) {
            e -= 1;
        }
        format!("{}…", &s[..e])
    } else {
        s.to_owned()
    }
}

#[test]
fn pfc_l4_perform_1289_1943() {
    if pob_testpaths::require(pob_testpaths::dump_dir(), "data/dump").is_none() {
        return;
    }
    assert!(testkit::init_default_data(), "data/dump does not load");
    let Some(inputs) = build_inputs() else { return };
    let mut clean = 0usize;
    let mut total = 0usize;
    let mut errors = Vec::new();
    let mut failed = Vec::new();
    let mut classes: BTreeMap<String, (usize, String)> = BTreeMap::new();
    let (mut order_only, mut model_only, mut replayed, mut leftover) = (0, 0, 0, 0);
    let mut upvalues_checked = 0usize;
    let mut notes = Vec::new();
    let mut unrun = 0usize;
    for (stem, input) in inputs {
        // the probe cache entry (or private run) is held until the build's dumps are loaded
        let b = match input.open().and_then(|s| load_build_dumps(s.state())) {
            Ok(b) => b,
            Err(e) => {
                errors.push(format!("{stem}: {e}"));
                total += 1;
                continue;
            }
        };
        total += 1;
        unrun += unrun_mismatches(&stem, &b);
        let o = run_build(&stem, &b);
        replayed += o.replayed;
        leftover += o.leftover;
        order_only += o.order_only;
        model_only += o.model_only;
        upvalues_checked += o.upvalues_checked;
        notes.extend(o.replay_notes.iter().map(|n| format!("{stem}: {n}")));
        if let Some(e) = o.error {
            errors.push(format!("{stem}: {e}"));
            continue;
        }
        if !o.party_alias_ok {
            failed.push(format!(
                "{stem} (env.partyMembers is not env.build.partyTab.actor)"
            ));
        } else if !o.upvalue_mismatches.is_empty() {
            failed.push(format!(
                "{stem} (closure upvalues: {})",
                o.upvalue_mismatches.join("; ")
            ));
        } else if o.mismatches.is_empty() && o.replay_notes.is_empty() {
            clean += 1;
        } else {
            failed.push(format!(
                "{stem} ({} mismatches, {} replay notes)",
                o.mismatches.len(),
                o.replay_notes.len()
            ));
            for m in &o.mismatches {
                let e = classes
                    .entry(format!("{:?} {}", m.kind, generalize(&m.path)))
                    .or_insert((0, String::new()));
                e.0 += 1;
                if e.1.is_empty() {
                    e.1 = format!(
                        "{stem} {}: pob={} ours={}",
                        m.path,
                        m.pob
                            .as_ref()
                            .map_or("-".to_owned(), |v| clip(&v.to_string())),
                        m.ours
                            .as_ref()
                            .map_or("-".to_owned(), |v| clip(&v.to_string()))
                    );
                }
            }
        }
    }
    eprintln!(
        "PF-C L4 perform:1289 -> perform:1943: {clean}/{total} builds clean, {} errors, \
         {replayed} callee calls replayed ({leftover} later ones unused), {order_only} \
         LUA-ORDER-only differences, {model_only} build-model-only differences (env.partyMembers.*)",
        errors.len()
    );
    for e in &errors {
        eprintln!("  error {e}");
    }
    for f in &failed {
        eprintln!("  mismatch {f}");
    }
    for n in notes.iter().take(30) {
        eprintln!("  replay note {n}");
    }
    let mut cls: Vec<_> = classes.into_iter().collect();
    cls.sort_by(|a, b| b.1 .0.cmp(&a.1 .0));
    for (c, (n, ex)) in cls.iter().take(40) {
        eprintln!("  {n:5} {c}\n        e.g. {ex}");
    }
    eprintln!(
        "  non-vacuity: {unrun} mismatches between the unrun perform:1289 state and perform:1943"
    );
    eprintln!("  closure upvalues (CCR-0079 §1) compared with perform:1691 / perform:1862: {upvalues_checked} closures");
    assert!(total > 0, "no build ran");
    assert!(
        unrun >= total,
        "the segment's effect is not visible in the diff"
    );
    // calc doc §9.4 L4: ≥ 98 % of the all57 hits (the remainder documented)
    assert!(
        clean * 100 >= total * 98,
        "PF-C L4: only {clean}/{total} builds match perform:1943"
    );
}
