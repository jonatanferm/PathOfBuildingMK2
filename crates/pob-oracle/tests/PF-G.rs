//! Unit PF-G acceptance (calc doc §11.1 row PF-G, §9.4 L4): inject the main env's
//! `perform:3497` dump, run the Rust segment (`calc_perform::ailments_dispatch::seg`) and diff
//! the state against PoB's `perform:out`, with the not-yet-ported callees the segment calls
//! replayed from their recorded `in → out` deltas (`pob_oracle::replay`):
//!
//! * `defence`, `buildDefenceEstimations`, `triggers`, `mirages`, `offence` (player and minion;
//!   the row's replay list), and
//! * the enemy's `doActorCharges` / `doActorMisc` (PF-B helpers, CalcPerform.lua 3624-3625),
//!   which the segment also calls and which are still stubs.
//!
//! Each call's `in`/`out` are the line probes around it (`corpus/PF-G/stages.toml`): a line
//! probe fires at most once per perform, so the main env's first hit (`pfg:*@main`) is the call.
//! `mirages`' return value is read off the probes (3799 fires only when it returned falsy). The
//! replays go through the segment's test seam `ailments_dispatch::replay_hook` (CCR-0077).
//!
//! At every replayed call the live state is also diffed against the call's `in` probe (before)
//! and `out` probe (after), which localises a mismatch. Verdicts per build: *clean*;
//! *replay-limited* (a replay could not reproduce its callee, the Rust segment added nothing;
//! CCR-0077 §6); *stub-blocked* (PoB ran the ally-Life block, whose helpers are unported stubs;
//! CCR-0077 §3); *divergent* (the Rust segment itself introduced mismatches: fails). Gate: no
//! divergent or failed build, and ≥ 98 % of the remaining (comparable) builds clean.
//!
//! Dumps are probed by this test into `target/tmp/pfg-probe/<stem>/` when absent (LuaJIT, the
//! machine-wide oracle lock). Knobs: `PFG_BUILDS=stem1,stem2`, `PFG_LIMIT=n`, `PFG_VERBOSE=1`
//! (every mismatch). A missing input skips with a message; with `POB_ORACLE_REQUIRE=1` (or `CI`)
//! it fails instead.
#![cfg(feature = "luajit")]

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use pob_calc::modules::calc_perform::ailments_dispatch::replay_hook;
use pob_calc::Val;
use pob_oracle::diff::{Mismatch, MismatchKind};
use pob_oracle::oracle::find_parity_dir;
use pob_oracle::replay::{Call, Replay};
use pob_oracle::schema::StateDump;
use pob_oracle::testkit;

fn skip(why: &str) {
    let required = std::env::var_os("POB_ORACLE_REQUIRE").is_some_and(|v| v == "1")
        || std::env::var_os("CI").is_some_and(|v| !v.is_empty() && v != "false" && v != "0");
    assert!(!required, "required input missing: {why}");
    eprintln!("SKIPPED: {why}");
}

fn stem(p: &Path) -> &str {
    p.file_stem().and_then(|s| s.to_str()).unwrap_or_default()
}

fn corpus(parity: &Path) -> Vec<PathBuf> {
    let mut v: Vec<PathBuf> = std::fs::read_dir(parity.join("builds"))
        .map(|r| {
            r.filter_map(Result::ok)
                .map(|e| e.path())
                .filter(|p| p.extension().is_some_and(|x| x == "xml"))
                .collect()
        })
        .unwrap_or_default();
    v.sort();
    if let Ok(list) = std::env::var("PFG_BUILDS") {
        let want: Vec<&str> = list.split(',').map(str::trim).collect();
        v.retain(|p| want.contains(&stem(p)));
    }
    if let Some(n) = std::env::var("PFG_LIMIT")
        .ok()
        .and_then(|s| s.parse::<usize>().ok())
    {
        v.truncate(n);
    }
    v
}

fn stages_file() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("corpus/PF-G/stages.toml")
}

/// The build's PF-G dumps (`perform:3497`, `perform:out`, `pfg:<line>`; main env), probed into
/// `target/tmp/pfg-probe/<stem>/state` when absent.
fn probe_dumps(xml: &Path) -> Result<PathBuf, String> {
    let stem = stem(xml);
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR"))
        .join("pfg-probe")
        .join(stem);
    let state = dir.join("state");
    if dir.join("hits.json").is_file() {
        return Ok(state);
    }
    let src = pob_oracle::oracle::find_pob_src().ok_or("no PoB checkout")?;
    let root = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("oracle-src");
    let patched = pob_oracle::patcher::patch(&src, Some(&stages_file()), &root)
        .map_err(|e| format!("patch: {e}"))?;
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let log = dir.with_extension("log");
    let status = std::process::Command::new(env!("CARGO_BIN_EXE_pob_oracle"))
        .arg("probe")
        .arg(xml)
        .arg("--out-dir")
        .arg(&dir)
        .arg("--patched")
        .arg(&patched.src)
        .args([
            "--select",
            "perform:3497@main,perform:out@main,pfg:*@main",
            "--no-provenance",
        ])
        .stdout(std::process::Stdio::null())
        .stderr(std::fs::File::create(&log).map_err(|e| e.to_string())?)
        .status()
        .map_err(|e| e.to_string())?;
    if status.success() {
        Ok(state)
    } else {
        let _ = std::fs::remove_file(dir.join("hits.json"));
        Err(format!("pob_oracle probe failed: see {}", log.display()))
    }
}

/// Every dump in `state`, by probe id (one per probe: the selection is `@main`).
fn load_dumps(state: &Path) -> Result<BTreeMap<String, StateDump>, String> {
    let mut out = BTreeMap::new();
    let rd = std::fs::read_dir(state).map_err(|e| format!("{}: {e}", state.display()))?;
    for e in rd.filter_map(Result::ok) {
        let p = e.path();
        let name = p.file_name().and_then(|n| n.to_str()).unwrap_or_default();
        if !(name.ends_with(".json") || name.ends_with(".json.zst")) {
            continue;
        }
        let d = StateDump::load(&p).map_err(|e| format!("{}: {e}", p.display()))?;
        out.insert(d.probe.clone(), d);
    }
    Ok(out)
}

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
enum Who {
    Player,
    Minion,
    Enemy,
}

/// One recorded call: the delta between two line probes, and its return value.
struct Recorded {
    din: StateDump,
    dout: StateDump,
    ret: Val,
}

/// The recorded calls of the segment, keyed by (callee, actor).
fn recorded_calls(dumps: &BTreeMap<String, StateDump>) -> BTreeMap<(&'static str, Who), Recorded> {
    let get = |line: u32| dumps.get(&format!("pfg:{line}")).cloned();
    let mut out = BTreeMap::new();
    let mut add = |callee: &'static str, who: Who, a: u32, b: Option<u32>, ret: Val| {
        let (Some(din), Some(dout)) = (get(a), b.and_then(get)) else {
            return;
        };
        out.insert((callee, who), Recorded { din, dout, ret });
    };
    add("doActorCharges", Who::Enemy, 3624, Some(3625), Val::Nil);
    add("doActorMisc", Who::Enemy, 3625, Some(3627), Val::Nil);
    add("defence", Who::Player, 3792, Some(3793), Val::Nil);
    add(
        "buildDefenceEstimations",
        Who::Player,
        3794,
        Some(3797),
        Val::Nil,
    );
    add("triggers", Who::Player, 3797, Some(3798), Val::Nil);
    // 3799 (offence) runs only when `calcs.mirages(env)` returned falsy
    let mirage_offence = dumps.contains_key("pfg:3799");
    add(
        "mirages",
        Who::Player,
        3798,
        Some(if mirage_offence { 3799 } else { 3802 }),
        if mirage_offence {
            Val::Nil
        } else {
            Val::Bool(true)
        },
    );
    add("offence", Who::Player, 3799, Some(3802), Val::Nil);
    add("defence", Who::Minion, 3803, Some(3804), Val::Nil);
    add(
        "buildDefenceEstimations",
        Who::Minion,
        3805,
        Some(3807),
        Val::Nil,
    );
    add("triggers", Who::Minion, 3807, Some(3808), Val::Nil);
    add("offence", Who::Minion, 3808, Some(3812), Val::Nil);
    out
}

#[derive(Default)]
struct Log {
    replayed: Vec<String>,
    problems: Vec<String>,
    /// Per replayed call: (call, mismatches of the live state against the call's `in` probe
    /// before the replay, against its `out` probe after it). The first localises a segment
    /// divergence (the Rust code before the call), the second a replay that did not land.
    steps: Vec<(String, usize, usize)>,
}

/// Mismatches (not `LUA-ORDER`) of the live state against a line-probe dump.
fn live_vs(dump: &StateDump, env: &pob_calc::env::EnvRef) -> usize {
    let locals = [(
        "env".to_owned(),
        Val::Obj(Rc::clone(env) as Rc<dyn std::any::Any>),
    )];
    testkit::diff_against(dump, Some(env), &locals, "")
        .iter()
        .filter(|m| !matches!(m.kind, MismatchKind::LuaOrder))
        .count()
}

/// CCR-0077 §4 workaround: the typed `Option` fields the segment sets that injection may have
/// marked absent (`allyLifeList` of each actor) count as present once `Some`.
fn unmark_present_options(env: &pob_calc::env::EnvRef) {
    let mut actors = vec![env.player.clone(), env.enemy.clone()];
    if let Some(m) = env.minion.get() {
        actors.push(m);
    }
    for a in actors {
        if a.allyLifeList.get().is_some() {
            pob_calc::probe::unmark_absent(pob_calc::probe::rc_ptr(&a), "allyLifeList");
        }
    }
}

/// Diff the live state at every replayed call (the verdict needs it); `PFG_STEP_DIFF=0` skips
/// it (faster, but every mismatch then counts as a divergence).
fn step_diff() -> bool {
    std::env::var_os("PFG_STEP_DIFF").map_or(true, |v| v != "0")
}

/// Install the replay hook for one build run; returns the shared log.
fn install_hook(calls: BTreeMap<(&'static str, Who), Recorded>) -> Rc<RefCell<Log>> {
    let log = Rc::new(RefCell::new(Log::default()));
    let l = Rc::clone(&log);
    let hook: replay_hook::Hook = Box::new(move |callee, env, actor| {
        // CCR-0077 §4: injection marks `player.allyLifeList` absent (nil at 3497); the segment's
        // `env.player.allyLifeList = { }` (3669) writes an empty table, which the exporter then
        // still treats as absent (`reg_absent && val_is_default`), so the replay's export ->
        // merge -> refill would drop it. A `Field<Option<_>>` that is `Some` is present.
        unmark_present_options(env);
        let who = if Rc::ptr_eq(actor, &env.player) {
            Who::Player
        } else if Rc::ptr_eq(actor, &env.enemy) {
            Who::Enemy
        } else if env.minion.get().is_some_and(|m| Rc::ptr_eq(actor, &m)) {
            Who::Minion
        } else {
            l.borrow_mut().problems.push(format!(
                "{callee}: actor is neither player, minion nor enemy"
            ));
            return None;
        };
        let key = calls
            .keys()
            .find(|(c, w)| *c == callee && *w == who)
            .copied();
        let Some(key) = key else {
            l.borrow_mut()
                .problems
                .push(format!("{callee}({who:?}) called, but PoB did not call it"));
            return None;
        };
        let rec = &calls[&key];
        let mut replay = Replay::default();
        replay.calls.insert(
            rec.din.hit.clone(),
            Call {
                din: rec.din.clone(),
                dout: rec.dout.clone(),
            },
        );
        let before = if step_diff() {
            live_vs(&rec.din, env)
        } else {
            0
        };
        match replay.apply_report(&rec.din.hit, env) {
            Ok(applied) => {
                if step_diff() {
                    unmark_present_options(env);
                    let after = live_vs(&rec.dout, env);
                    l.borrow_mut()
                        .steps
                        .push((format!("{callee}({who:?})"), before, after));
                }
                let mut lg = l.borrow_mut();
                if !applied.notes.is_empty() {
                    lg.problems.push(format!(
                        "{callee}({who:?}) replay notes: {:?}",
                        applied.notes
                    ));
                }
                lg.replayed.push(format!("{callee}({who:?})"));
            }
            Err(e) => l
                .borrow_mut()
                .problems
                .push(format!("{callee}({who:?}) replay failed: {e}")),
        }
        Some(rec.ret.clone())
    });
    replay_hook::set(Some(hook));
    log
}

struct BuildResult {
    mismatches: Vec<Mismatch>,
    replayed: Vec<String>,
    problems: Vec<String>,
    stubs: Vec<String>,
    /// Non-vacuity: state changes PoB made in the segment's own code (not a replayed call),
    /// per region (see [`OWN_REGIONS`]); the Rust segment had to reproduce them.
    own: Vec<usize>,
    /// (call, mismatches before vs `in`, after vs `out`) per replayed call.
    steps: Vec<(String, usize, usize)>,
    /// PoB filled `env.player.allyLifeList` (CalcPerform.lua 3670-3789 ran): that path needs
    /// PF-B's `allyLifePools` (and `initMinionModDB`, `addMinionModifiers`), PF-A's
    /// `doActorAttribsConditions` / `doActorLifeMana` and OF-A's `calcTotemLife`, all stubs.
    needs_ally_life: bool,
}

/// How a build's run ended.
#[derive(PartialEq, Eq)]
enum Verdict {
    /// No mismatch at the end and every replay landed.
    Clean,
    /// A replay could not reproduce its callee's `out` (it added mismatches) and the Rust
    /// segment added none of its own before any call or after the last one: the remaining
    /// mismatches are the replay's (calc doc §9.2 Replay limits; CCR-0077 §6).
    ReplayLimited(String),
    /// The Rust segment diverged in the ally-Life block (3667-3789) of a build that needs it:
    /// the block's helpers are unported stubs (see `BuildResult::needs_ally_life`).
    StubBlocked(String),
    /// The Rust segment itself diverged (new mismatches between two calls, or at the end).
    Divergent(String),
}

fn verdict(r: &BuildResult, real: usize) -> Verdict {
    if real == 0 && r.problems.is_empty() && r.steps.iter().all(|(_, b, a)| *b == 0 && *a == 0) {
        return Verdict::Clean;
    }
    if !r.problems.is_empty() {
        return Verdict::Divergent(r.problems.join("; "));
    }
    let mut prev = 0usize;
    let mut limited = Vec::new();
    for (call, before, after) in &r.steps {
        if *before > prev {
            let why = format!("{} new mismatches before {call}", before - prev);
            // the first call after the ally-Life block is `defence(Player)`
            if r.needs_ally_life && call == "defence(Player)" {
                return Verdict::StubBlocked(why);
            }
            return Verdict::Divergent(why);
        }
        if after > before {
            limited.push(format!("{call} +{}", after - before));
        }
        prev = *after;
    }
    if real > prev {
        let why = format!("{} new mismatches after the last call", real - prev);
        if limited.is_empty() {
            return Verdict::Divergent(why);
        }
        // the tail (party exports, gem level/quality, cacheData) reads the state a failed
        // replay left (e.g. the wrong `env.player.mainSkill` after `mirages`): not comparable
        limited.push(format!("tail not comparable: {why}"));
    }
    if limited.is_empty() {
        Verdict::Divergent(format!("{real} mismatches"))
    } else {
        Verdict::ReplayLimited(limited.join(", "))
    }
}

/// The stretches of the segment that are PF-G's own code: (label, from probe, to probe).
const OWN_REGIONS: &[(&str, &str, &str)] = &[
    ("ailments 3497-3622", "perform:3497", "pfg:3624"),
    (
        "exposure/consecrated/ally Life 3627-3790",
        "pfg:3627",
        "pfg:3792",
    ),
    (
        "party exports/gem level/cacheData 3812-3989",
        "pfg:3812",
        "perform:out",
    ),
];

/// State differences (env + GlobalCache) between two dumps of the same perform.
fn state_changes(a: &StateDump, b: &StateDump) -> usize {
    let strip = |d: &StateDump| {
        let mut r = d.root.clone();
        if let Some(m) = r.as_object_mut() {
            m.remove("locals");
        }
        pob_oracle::schema::resolve_ids(&r)
    };
    pob_oracle::diff::diff_trees(
        &strip(a),
        &strip(b),
        &pob_oracle::diff::DiffOptions::strict(),
    )
    .len()
}

/// The active spec's `treeVersion` attribute of a build XML (`<Tree activeSpec="i">`, the i-th
/// `<Spec>`).
fn xml_tree_version(xml: &str) -> Option<String> {
    let attr = |tag: &str, name: &str| -> Option<String> {
        let key = format!("{name}=\"");
        let i = tag.find(&key)? + key.len();
        let j = tag[i..].find('"')? + i;
        Some(tag[i..j].to_owned())
    };
    let tree_start = xml.find("<Tree")?;
    let tree_tag = &xml[tree_start..tree_start + xml[tree_start..].find('>')?];
    let active: usize = attr(tree_tag, "activeSpec")
        .and_then(|s| s.parse().ok())
        .unwrap_or(1);
    let mut rest = &xml[tree_start..];
    let mut k = 0;
    while let Some(i) = rest.find("<Spec") {
        let tag = &rest[i..i + rest[i..].find('>')?];
        k += 1;
        if k == active {
            return attr(tag, "treeVersion");
        }
        rest = &rest[i + 5..];
    }
    None
}

/// `"$id" -> "$uuid"` of every active skill in a dump root.
fn skill_uuids(j: &serde_json::Value, out: &mut BTreeMap<u64, String>) {
    match j {
        serde_json::Value::Object(m) => {
            if let (Some(id), Some(u)) = (
                m.get("$id").and_then(serde_json::Value::as_u64),
                m.get("$uuid").and_then(serde_json::Value::as_str),
            ) {
                out.insert(id, u.to_owned());
            }
            for v in m.values() {
                skill_uuids(v, out);
            }
        }
        serde_json::Value::Array(a) => {
            for v in a {
                skill_uuids(v, out);
            }
        }
        _ => {}
    }
}

/// The build-model state the segment reads that a pob-state dump does not carry (the build is
/// the `{"$object": "Build"}` marker and BM-A's XML loader is still a stub), filled in from
/// the XML and the dump: the active spec's `treeVersion` (CalcPerform.lua:3627) and, for
/// `cacheSkillUUID` (Common.lua:913-918), `skillsTab.socketGroupList` with the main skill's
/// injected socket group at the index PoB's uuid names (the other slots are empty groups).
fn fill_build_model(
    xml: &Path,
    din: &StateDump,
    env: &pob_calc::env::EnvRef,
) -> Result<(), String> {
    let text = std::fs::read_to_string(xml).map_err(|e| e.to_string())?;
    let tv = xml_tree_version(&text).ok_or("no treeVersion in the build XML")?;
    env.spec
        .get()
        .treeVersion
        .set(pob_lua::Name::from(tv.as_str()));
    let mut uuids = BTreeMap::new();
    skill_uuids(&din.root, &mut uuids);
    let ms = &din.root["env"]["player"]["mainSkill"];
    let id = ms
        .get("$id")
        .or_else(|| ms.get("$ref"))
        .and_then(serde_json::Value::as_u64);
    let (Some(uuid), Some(skill)) = (id.and_then(|i| uuids.get(&i)), env.player.mainSkill.get())
    else {
        return Ok(());
    };
    let group_idx: usize = uuid
        .rsplit('_')
        .next()
        .and_then(|s| s.parse().ok())
        .ok_or("bad uuid")?;
    if let Some(group) = skill.socketGroup.get() {
        let list: Vec<pob_calc::model::SocketGroupRef> = (1..=group_idx)
            .map(|i| {
                if i == group_idx {
                    group.clone()
                } else {
                    Rc::new(pob_calc::model::SocketGroup::default())
                }
            })
            .collect();
        env.build
            .skillsTab
            .socketGroupList
            .set(pob_lua::Shared::new(list));
    }
    Ok(())
}

/// L4 for one build: inject `perform:3497`, run the segment with replays, diff `perform:out`.
fn run_build(
    xml: &Path,
    stem: &str,
    dumps: &BTreeMap<String, StateDump>,
) -> Result<BuildResult, String> {
    let din = dumps.get("perform:3497").ok_or("no perform:3497 dump")?;
    let dout = dumps.get("perform:out").ok_or("no perform:out dump")?;
    let calls = recorded_calls(dumps);
    // PoB's calls must all be replayed by the Rust run: remember them
    let expected: Vec<String> = calls.keys().map(|(c, w)| format!("{c}({w:?})")).collect();
    let build = testkit::empty_build();
    let inj = pob_oracle::inject::inject(din, &build)?;
    let env0 = inj.env.clone().ok_or("no env in perform:3497")?;
    fill_build_model(xml, din, &env0)?;
    let locals: Vec<(String, Val)> = inj.locals.into_iter().collect();
    pob_calc::stub::stub_reset();
    let log = install_hook(calls);
    let run = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        pob_calc::probe::run_segment(&din.hit, &locals)
    }));
    replay_hook::set(None);
    let stubs: Vec<String> = pob_calc::stub::stub_report()
        .iter()
        .map(|(site, n)| format!("{site} x{n}"))
        .collect();
    let mut after = match run {
        Ok(Ok((flow, after))) => {
            if flow != pob_calc::Flow::Normal {
                return Err(format!("segment returned {flow:?}"));
            }
            after
        }
        Ok(Err(e)) => return Err(format!("run: {e}")),
        Err(p) => {
            let msg = p
                .downcast_ref::<pob_lua::LuaError>()
                .map(|e| e.0.clone())
                .or_else(|| p.downcast_ref::<String>().cloned())
                .or_else(|| p.downcast_ref::<&str>().map(|s| (*s).to_owned()))
                .unwrap_or_else(|| "panic".to_owned());
            return Err(format!("run panicked: {msg}"));
        }
    };
    // `perform:out` dumps perform's (empty) packed results
    let ret = pob_calc::Table::new();
    ret.set("n", 0.0);
    after.push(("ret".to_owned(), Val::Table(ret)));
    let env = after
        .iter()
        .find(|(k, _)| k == "env")
        .and_then(|(_, v)| v.obj::<pob_calc::env::Env>())
        .or(inj.env);
    if let Some(e) = &env {
        unmark_present_options(e);
    }
    let mismatches = testkit::diff_against(dout, env.as_ref(), &after, stem);
    let log = Rc::try_unwrap(log)
        .map(RefCell::into_inner)
        .unwrap_or_else(|l| std::mem::take(&mut *l.borrow_mut()));
    if std::env::var_os("PFG_VERBOSE").is_some() {
        for (call, before, after) in &log.steps {
            eprintln!(
                "  {stem}: step {call}: {before} mismatches before (vs in), {after} after (vs out)"
            );
        }
    }
    let mut problems = log.problems;
    for e in &expected {
        if !log.replayed.contains(e) {
            problems.push(format!("PoB called {e}; the Rust segment did not"));
        }
    }
    let needs_ally_life = dumps.get("pfg:3792").is_some_and(|d| {
        d.root["env"]["player"]["allyLifeList"]
            .as_object()
            .is_some_and(|m| !m.is_empty())
    });
    let own = OWN_REGIONS
        .iter()
        .map(|(_, a, b)| match (dumps.get(*a), dumps.get(*b)) {
            (Some(a), Some(b)) => state_changes(a, b),
            _ => 0,
        })
        .collect();
    Ok(BuildResult {
        mismatches,
        replayed: log.replayed,
        problems,
        stubs,
        own,
        steps: log.steps,
        needs_ally_life,
    })
}

fn clip(s: &str) -> String {
    if s.len() > 160 {
        let end = s.char_indices().nth(160).map_or(s.len(), |(i, _)| i);
        format!("{}…", &s[..end])
    } else {
        s.to_owned()
    }
}

/// `a.b[3].c` → `a.b[#].c` (groups mismatches by shape).
fn generalize(path: &str) -> String {
    let mut out = String::new();
    let mut chars = path.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '['
            && chars
                .peek()
                .is_some_and(|d| d.is_ascii_digit() || *d == '-' || *d == '@')
        {
            out.push_str("[#]");
            for d in chars.by_ref() {
                if d == ']' {
                    break;
                }
            }
        } else {
            out.push(c);
        }
    }
    out
}

/// L4 over the corpus (all57 by default). Gate (calc doc §9.4 L4): every build clean but for
/// `LUA-ORDER` reports, except at most 2% of the builds (documented in known_diffs.toml).
#[test]
fn pfg_l4_perform_3497_to_out() {
    if !testkit::init_default_data() {
        return skip("no data dump (data/dump)");
    }
    let Some(parity) = find_parity_dir() else {
        return skip("no .parity directory");
    };
    if pob_oracle::oracle::find_pob_src().is_none() {
        return skip("no PoB checkout");
    }
    let builds = corpus(&parity);
    if builds.is_empty() {
        return skip("no .parity/builds/*.xml");
    }
    let verbose = std::env::var_os("PFG_VERBOSE").is_some();
    let mut clean = Vec::new();
    let mut dirty: Vec<String> = Vec::new();
    let mut limited: Vec<String> = Vec::new();
    let mut blocked: Vec<String> = Vec::new();
    let mut failed: Vec<String> = Vec::new();
    let mut orders = 0usize;
    let mut by_path: BTreeMap<String, (usize, String)> = BTreeMap::new();
    let mut replay_counts: BTreeMap<String, usize> = BTreeMap::new();
    let mut stub_counts: BTreeMap<String, usize> = BTreeMap::new();
    let mut own_builds: Vec<(usize, usize)> = Vec::new();
    for xml in &builds {
        let stem = stem(xml);
        let dumps = match probe_dumps(xml).and_then(|s| load_dumps(&s)) {
            Ok(d) => d,
            Err(e) => {
                failed.push(format!("{stem}: {e}"));
                continue;
            }
        };
        match run_build(xml, stem, &dumps) {
            Ok(r) => {
                for (i, n) in r.own.iter().enumerate() {
                    if own_builds.len() <= i {
                        own_builds.push((0, 0));
                    }
                    if *n > 0 {
                        own_builds[i].0 += 1;
                        own_builds[i].1 += n;
                    }
                }
                for s in &r.replayed {
                    *replay_counts.entry(s.clone()).or_default() += 1;
                }
                for s in &r.stubs {
                    let site = s.rsplit_once(" x").map_or(s.as_str(), |(a, _)| a);
                    *stub_counts.entry(site.to_owned()).or_default() += 1;
                }
                let real: Vec<&Mismatch> = r
                    .mismatches
                    .iter()
                    .filter(|m| !matches!(m.kind, MismatchKind::LuaOrder))
                    .collect();
                orders += r.mismatches.len() - real.len();
                for m in &real {
                    let key = format!("{:?} {}", m.kind, generalize(&m.path));
                    let e = by_path.entry(key).or_insert_with(|| {
                        (
                            0,
                            format!(
                                "{stem}: {} pob={} ours={}",
                                m.path,
                                m.pob.as_ref().map_or("-".into(), |x| clip(&x.to_string())),
                                m.ours.as_ref().map_or("-".into(), |x| clip(&x.to_string()))
                            ),
                        )
                    });
                    e.0 += 1;
                    if verbose {
                        eprintln!(
                            "  {stem}: {} {:?} pob={:?} ours={:?}",
                            m.path, m.kind, m.pob, m.ours
                        );
                    }
                }
                for p in &r.problems {
                    eprintln!("  {stem}: {p}");
                }
                match verdict(&r, real.len()) {
                    Verdict::Clean => {
                        eprintln!("PF-G {stem}: clean");
                        clean.push(stem.to_owned());
                    }
                    Verdict::ReplayLimited(why) => {
                        eprintln!("PF-G {stem}: replay-limited ({why})");
                        limited.push(format!("{stem} ({} mismatches; {why})", real.len()));
                    }
                    Verdict::StubBlocked(why) => {
                        eprintln!("PF-G {stem}: stub-blocked ally Life ({why})");
                        blocked.push(format!("{stem} ({} mismatches; {why})", real.len()));
                    }
                    Verdict::Divergent(why) => {
                        eprintln!("PF-G {stem}: DIVERGENT ({why})");
                        dirty.push(format!("{stem} ({} mismatches; {why})", real.len()));
                    }
                }
            }
            Err(e) => {
                eprintln!("PF-G {stem}: FAILED ({e})");
                failed.push(format!("{stem}: {e}"));
            }
        }
    }
    let total = clean.len() + limited.len() + blocked.len() + dirty.len() + failed.len();
    eprintln!(
        "PF-G L4 perform:3497 -> perform:out: {} builds, {} clean, {} replay-limited, {} stub-blocked (ally Life), {} segment-divergent, {} failed, {} LUA-ORDER reports",
        total,
        clean.len(),
        limited.len(),
        blocked.len(),
        dirty.len(),
        failed.len(),
        orders
    );
    for s in &limited {
        eprintln!("  replay-limited: {s}");
    }
    for s in &blocked {
        eprintln!("  stub-blocked: {s}");
    }
    for s in &dirty {
        eprintln!("  DIVERGENT: {s}");
    }
    for f in &failed {
        eprintln!("  failed: {f}");
    }
    let mut paths: Vec<_> = by_path.iter().collect();
    paths.sort_by(|a, b| b.1 .0.cmp(&a.1 .0));
    for (p, (n, ex)) in paths.iter().take(40) {
        eprintln!("  {n:6} {p}\n         e.g. {ex}");
    }
    eprintln!("  segment-own state changes PoB made (builds with changes, total changes):");
    for (i, (label, _, _)) in OWN_REGIONS.iter().enumerate() {
        let (b, n) = own_builds.get(i).copied().unwrap_or_default();
        eprintln!("    {b:4} builds, {n:6} changes  {label}");
    }
    eprintln!("  replayed calls (builds):");
    for (k, n) in &replay_counts {
        eprintln!("    {n:4} {k}");
    }
    eprintln!("  stubs hit (builds):");
    for (k, n) in &stub_counts {
        eprintln!("    {n:4} {k}");
    }
    assert!(total > 0, "no builds");
    // The segment's own code may never diverge, and every build must run.
    assert!(
        dirty.is_empty() && failed.is_empty(),
        "PF-G L4: {} builds where the Rust segment diverged, {} failed",
        dirty.len(),
        failed.len()
    );
    // §9.4 L4: ≥ 98% of the hits clean, counted over the builds whose replays all landed (a
    // replay-limited build cannot be compared past the callee the replay could not reproduce).
    let comparable = total - limited.len() - blocked.len();
    let need = (comparable as f64 * 0.98).ceil() as usize;
    assert!(
        clean.len() >= need,
        "PF-G L4: {} of {comparable} comparable builds clean (need {need})",
        clean.len()
    );
}

/// The replay seam is inert without a hook and restores itself: a hook that declines every
/// call (`None`) leaves the Rust callees running, and `set(None)` removes it.
#[test]
fn pfg_replay_hook_roundtrip() {
    let calls = Rc::new(RefCell::new(0));
    let c = Rc::clone(&calls);
    let prev = replay_hook::set(Some(Box::new(move |_, _, _| {
        *c.borrow_mut() += 1;
        None
    })));
    assert!(prev.is_none());
    let back = replay_hook::set(None);
    assert!(back.is_some());
    assert!(replay_hook::set(None).is_none());
    assert_eq!(*calls.borrow(), 0);
}
