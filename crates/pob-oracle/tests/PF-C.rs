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
//! `setup_flasks::callee_replay`, CCR-0078 §2).
//!
//! Test-environment gaps, each worked around here and recorded in CCR-0078:
//! * §3: the pob-state/1 item dump has no `buffModList`; the flasks' lists are
//!   restored from the `pfc:calcFlaskMods` line dumps (their `buffModList` local).
//! * §4: the injected graph has no passive tree (PT, BM-A), so `modLib.mergeKeystones`
//!   (ported, CT) cannot resolve keystones; its recorded delta is replayed after it.
//! * `env.partyMembers` is the build model's `partyTab.actor` (CalcPerform.lua:1310);
//!   the model is empty until BM-A (`Build:LoadDB` is a stub), so its contents are
//!   not compared ([`MODEL_ONLY`]) and the aliasing is checked instead.
//!
//! Dumps: `crates/pob-oracle/corpus/PF-C/probe.sh` probes every `.parity/builds`
//! build with `corpus/PF-C/stages.toml` into `target/tmp/pfc-probe/<stem>/`
//! (`$PFC_PROBE_DIR` overrides). Without dumps the test prints that command and
//! skips (POB_ORACLE_REQUIRE=1 / CI make that a failure).
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
/// needs the passive tree (CCR-0078 §4); the segment reports every call of it.
const REPLAYED: &[&str] = &[
    "createMinionSkills",
    "initMinionModDB",
    "applyEnemyModifiers",
    "mergeKeystones",
];

/// Paths whose contents come from the (unloaded) build model, not from the segment.
const MODEL_ONLY: &[&str] = &["env.partyMembers."];

fn skip(why: &str) {
    let required = std::env::var_os("POB_ORACLE_REQUIRE").is_some_and(|v| v == "1")
        || std::env::var_os("CI").is_some_and(|v| !v.is_empty() && v != "false" && v != "0");
    assert!(!required, "required input missing: {why}");
    eprintln!("SKIPPED: {why}");
}

fn probe_root() -> PathBuf {
    std::env::var_os("PFC_PROBE_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("pfc-probe"))
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
    let dest = destTable.table(destKey).expect("destTable[destKey] is a table");
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
    /// callee → its recorded calls and the hits the segment's perform made, in call order.
    replays: Rc<BTreeMap<&'static str, (Replay, Vec<String>)>>,
    /// item id → `item.buffModList` as PoB had it (from the `pfc:calcFlaskMods` dumps).
    buffModLists: BTreeMap<i64, Vec<Mod>>,
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
/// `calcs.perform` call (CCR-0078 §3: the pob-state/1 item dump leaves the field out).
fn buff_mod_lists(
    dumps: &BTreeMap<String, StateDump>,
    din: &StateDump,
) -> Result<BTreeMap<i64, Vec<Mod>>, String> {
    let mut out = BTreeMap::new();
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
            let (m, _) = pob_oracle::mod_json::decode_mod(e).map_err(|x| format!("{}: {x}", d.hit))?;
            mods.push(m);
        }
        out.insert(id as i64, mods);
    }
    Ok(out)
}

fn load_build_dumps(dir: &Path) -> Result<BuildDumps, String> {
    let state = dir.join("state");
    let mut dumps = BTreeMap::new();
    for e in std::fs::read_dir(&state).map_err(|e| format!("{}: {e}", state.display()))? {
        let p = e.map_err(|e| e.to_string())?.path();
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
    let buffModLists = buff_mod_lists(&dumps, &din)?;
    Ok(BuildDumps {
        din,
        dout,
        replays: Rc::new(replays),
        buffModLists,
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
    error: Option<String>,
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

/// CalcPerform.lua 1310-1311: `env.partyMembers` and `env.player.partyMembers` are the build's
/// `partyTab.actor`.
fn party_alias_ok(env: &EnvRef) -> bool {
    let party = &env.build.partyTab.actor;
    env.partyMembers.get().is_some_and(|p| Rc::ptr_eq(&p, party))
        && env
            .player
            .partyMembers
            .get()
            .upgrade()
            .is_some_and(|p| Rc::ptr_eq(&p, party))
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
        .filter(|m| m.kind != MismatchKind::LuaOrder && !MODEL_ONLY.iter().any(|p| m.path.starts_with(p)))
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
    // CCR-0078 §3: restore the flasks' buffModList the item dump leaves out (again after
    // every replay: `apply_onto` refills the items from the export, which lacks the field)
    let lists = Rc::new(b.buffModLists.clone());
    restore_buff_mod_lists(&env, &lists);

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
        callee_replay::set_hook(Some(Box::new(move |name, args| {
            if name == "mergeBuff" {
                let (Some(src), Some(dest), Some(key)) = (
                    args.first().and_then(Val::table),
                    args.get(1).and_then(Val::table),
                    args.get(2).and_then(Val::str),
                ) else {
                    log.borrow_mut().1.push("mergeBuff: bad arguments".to_owned());
                    return;
                };
                merge_buff_standin(&src, &dest, Name::from(&*key));
                return;
            }
            let hit = queues.borrow_mut().get_mut(name).and_then(VecDeque::pop_front);
            let Some(hit) = hit else {
                log.borrow_mut().1.push(format!("{name}: no recorded call left"));
                return;
            };
            let r = replays[name].0.apply_report(&hit, &env);
            restore_buff_mod_lists(&env, &lists);
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
    out.party_alias_ok = party_alias_ok(&env);
    let mm = testkit::diff_against(&b.dout, Some(&env), &after, stem);
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
    if !testkit::init_default_data() {
        return skip("no data dump (data/dump)");
    }
    let root = probe_root();
    let mut dirs: Vec<PathBuf> = std::fs::read_dir(&root)
        .map(|rd| {
            rd.filter_map(Result::ok)
                .map(|e| e.path())
                .filter(|p| p.join("hits.json").is_file())
                .collect()
        })
        .unwrap_or_default();
    dirs.sort();
    if let Ok(only) = std::env::var("PFC_ONLY") {
        dirs.retain(|d| d.file_name().is_some_and(|n| n.to_string_lossy().contains(&only)));
    }
    if dirs.is_empty() {
        return skip(&format!(
            "no PF-C dumps under {} (fill with: crates/pob-oracle/corpus/PF-C/probe.sh)",
            root.display()
        ));
    }
    let mut clean = 0usize;
    let mut total = 0usize;
    let mut errors = Vec::new();
    let mut failed = Vec::new();
    let mut classes: BTreeMap<String, (usize, String)> = BTreeMap::new();
    let (mut order_only, mut model_only, mut replayed, mut leftover) = (0, 0, 0, 0);
    let mut notes = Vec::new();
    let mut unrun = 0usize;
    for dir in &dirs {
        let stem = dir.file_name().unwrap().to_string_lossy().into_owned();
        let b = match load_build_dumps(dir) {
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
        notes.extend(o.replay_notes.iter().map(|n| format!("{stem}: {n}")));
        if let Some(e) = o.error {
            errors.push(format!("{stem}: {e}"));
            continue;
        }
        if !o.party_alias_ok {
            failed.push(format!("{stem} (env.partyMembers is not env.build.partyTab.actor)"));
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
                        m.pob.as_ref().map_or("-".to_owned(), |v| clip(&v.to_string())),
                        m.ours.as_ref().map_or("-".to_owned(), |v| clip(&v.to_string()))
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
    eprintln!("  non-vacuity: {unrun} mismatches between the unrun perform:1289 state and perform:1943");
    assert!(total > 0, "no build ran");
    assert!(unrun >= total, "the segment's effect is not visible in the diff");
    // calc doc §9.4 L4: ≥ 98 % of the all57 hits (the remainder documented)
    assert!(
        clean * 100 >= total * 98,
        "PF-C L4: only {clean}/{total} builds match perform:1943"
    );
}
