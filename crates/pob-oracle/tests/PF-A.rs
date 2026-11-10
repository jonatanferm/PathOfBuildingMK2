//! Unit PF-A acceptance (calc doc §11.1 row PF-A): function probes on the
//! CalcPerform.lua 1-635 helpers `getCachedOutputValue`, `mergeBuff`,
//! `doActorLifeMana`, `doActorAttribsConditions`, `doActorLifeManaReservation`,
//! `determineCursePriority` and `applyEnemyModifiers`.
//!
//! For every all57 build, PoB is probed (patched with
//! `corpus/PF-A/stages.toml`, whose `[[function]]` entries wrap the helpers;
//! `getCachedOutputValue` is a built-in function probe) and each helper's first
//! call in the build's main env is dumped at `<fn>:in#n` and `<fn>:out#n`
//! (selection `<fn>:*@main`). The test injects `:in`, runs the Rust function
//! (`pob_calc::modules::calc_perform::actor_a::probe_call`) on the injected
//! locals, and diffs env, GlobalCache, the parameters and the return values
//! (`ret`) against `:out` (`testkit::diff_against`, calc doc §9.2 tolerance).
//!
//! Non-vacuity: for each pair the test also diffs the untouched injection
//! against `:out`; a pair whose `:in` already equals `:out` is counted as
//! "no-op" (the function changed nothing PoB dumps).
//!
//! Builds: all57 (`.parity/builds`) plus the unit corpus `corpus/PF-A/*.xml`.
//! Dumps go to `target/tmp/pfa-probe/<stem>/` (needs LuaJIT, feature
//! `luajit`, and the machine-wide oracle lock). Knobs: `PFA_BUILDS=stem1,stem2`,
//! `PFA_LIMIT=n`, `PFA_VERBOSE=1` (every mismatch), `PFA_REPROBE=1`. A missing
//! input skips with a message; with `POB_ORACLE_REQUIRE=1` (or `CI`) it fails.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use pob_calc::Val;
use pob_oracle::diff::Mismatch;
use pob_oracle::oracle::find_parity_dir;
use pob_oracle::schema::StateDump;
use pob_oracle::testkit;

const FNS: &[&str] = &[
    "getCachedOutputValue",
    "mergeBuff",
    "doActorLifeMana",
    "doActorAttribsConditions",
    "doActorLifeManaReservation",
    "determineCursePriority",
    "applyEnemyModifiers",
];

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
    // the unit corpus: synthetic builds for branches all57 does not reach
    // (pfa_cached_outputs: Blight / Penance Brand of Dissipation with skillPart 2 call
    // getCachedOutputValue, CalcPerform.lua:2281/2288)
    let mut unit: Vec<PathBuf> =
        std::fs::read_dir(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("corpus/PF-A"))
            .map(|r| {
                r.filter_map(Result::ok)
                    .map(|e| e.path())
                    .filter(|p| p.extension().is_some_and(|x| x == "xml"))
                    .collect()
            })
            .unwrap_or_default();
    unit.sort();
    v.extend(unit);
    if let Ok(list) = std::env::var("PFA_BUILDS") {
        let want: Vec<&str> = list.split(',').map(str::trim).collect();
        v.retain(|p| want.contains(&stem(p)));
    }
    if let Some(n) = std::env::var("PFA_LIMIT")
        .ok()
        .and_then(|s| s.parse::<usize>().ok())
    {
        v.truncate(n);
    }
    v
}

fn stages_file() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("corpus/PF-A/stages.toml")
}

/// The patched PoB copy with the PF-A function probes (built once per test process).
#[cfg(feature = "luajit")]
fn patched_src() -> Option<PathBuf> {
    use std::sync::OnceLock;
    static P: OnceLock<Option<PathBuf>> = OnceLock::new();
    P.get_or_init(|| {
        let src = pob_oracle::oracle::find_pob_src()?;
        let root = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("oracle-src");
        match pob_oracle::patcher::patch(&src, Some(&stages_file()), &root) {
            Ok(p) => Some(p.src),
            Err(e) => {
                eprintln!("patch failed: {e}");
                None
            }
        }
    })
    .clone()
}

/// The `:in`/`:out` dumps of the PF-A helpers in a build's main env, probed into
/// `target/tmp/pfa-probe/<stem>/` when absent.
#[cfg(feature = "luajit")]
fn probe_dumps(xml: &Path) -> Result<PathBuf, String> {
    let select: Vec<String> = FNS.iter().map(|f| format!("{f}:*@main")).collect();
    probe_into(xml, "pfa-probe", &select)
}

/// Hits `#1..#HIT_SAMPLE` of every PF-A helper (any env: initEnv/perform runs of every
/// mode, buildActiveSkill and calcFullDPS envs, minions), probed into
/// `target/tmp/pfa-probe-hits/<stem>/`.
#[cfg(feature = "luajit")]
fn probe_hit_sample(xml: &Path) -> Result<PathBuf, String> {
    let mut select = Vec::new();
    for f in FNS {
        for n in 1..=HIT_SAMPLE {
            select.push(format!("{f}:*#{n}"));
        }
    }
    probe_into(xml, "pfa-probe-hits", &select)
}

const HIT_SAMPLE: usize = 12;

/// Builds whose `applyEnemyModifiers` calls `#LATE_HITS` (the main-env performs) include
/// doActorMisc's call (`clearCache` nil, CalcPerform.lua:976) on a cache the flask call
/// (`clearCache` true) of the same perform already filled, while EnemyModifier mods that
/// were inactive then (ActorCondition `Marked`, `Ignited`, ...) are now listed before the
/// cached ones: the `cache[mod]` hit path on keys `setSource` re-sourced (CCR-0085 §3-4).
const LATE_BUILDS: &[&str] = &["allflame_exp_030", "allflame_exp_048"];
const LATE_HITS: std::ops::RangeInclusive<usize> = 13..=24;

/// `applyEnemyModifiers` hits [`LATE_HITS`], probed into `target/tmp/pfa-probe-late/<stem>/`.
#[cfg(feature = "luajit")]
fn probe_late_hits(xml: &Path) -> Result<PathBuf, String> {
    let select: Vec<String> = LATE_HITS
        .map(|n| format!("applyEnemyModifiers:*#{n}"))
        .collect();
    probe_into(xml, "pfa-probe-late", &select)
}

#[cfg(feature = "luajit")]
fn probe_into(xml: &Path, sub: &str, select: &[String]) -> Result<PathBuf, String> {
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR"))
        .join(sub)
        .join(stem(xml));
    let state = dir.join("state");
    if dir.join("hits.json").is_file() && std::env::var_os("PFA_REPROBE").is_none() {
        return Ok(state);
    }
    let patched = patched_src().ok_or("no patched PoB source")?;
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let log = dir.with_extension("log");
    let status = std::process::Command::new(env!("CARGO_BIN_EXE_pob_oracle"))
        .arg("probe")
        .arg(xml)
        .arg("--out-dir")
        .arg(&dir)
        .arg("--patched")
        .arg(&patched)
        .arg("--select")
        .arg(select.join(","))
        .arg("--no-provenance")
        .arg("--max-dumps")
        .arg("1000")
        .stdout(std::process::Stdio::null())
        .stderr(std::fs::File::create(&log).map_err(|e| e.to_string())?)
        .status()
        .map_err(|e| e.to_string())?;
    if status.success() {
        Ok(state)
    } else {
        Err(format!("pob_oracle probe failed (see {})", log.display()))
    }
}

/// Pairs whose mismatches are an oracle gap, not a PF-A port difference: (build, function,
/// reason). A listed pair that comes out clean fails the test (the entry is stale).
const KNOWN_DIFFS: &[(&str, &str, &str)] = &[
    (
        "allflame_exp_016",
        "doActorLifeMana",
        "CCR-0085 §5: `15% increased Life` (Tree:41415) carries an ItemCondition searchCond \
         tag on the body armour; pob-state/1 dumps no item mod lines, so \
         Item:FindModifierSubstring on the injected Kaom's Heart finds nothing",
    ),
    (
        "allflame_exp_016",
        "doActorAttribsConditions",
        "CCR-0085 §5: as doActorLifeMana (it ends by calling doActorLifeMana)",
    ),
    (
        "allflame_exp_049",
        "doActorLifeMana",
        "CCR-0085 §5: `15% increased Life` (Tree:44316) carries the same ItemCondition \
         searchCond tag; the body armour's `Regenerate 10% Life …` line is not in the dump",
    ),
    (
        "allflame_exp_049",
        "doActorAttribsConditions",
        "CCR-0085 §5: as doActorLifeMana (it ends by calling doActorLifeMana)",
    ),
];

fn known_diff(stem: &str, f: &str) -> Option<&'static str> {
    KNOWN_DIFFS
        .iter()
        .find(|(b, g, _)| *b == stem && *g == f)
        .map(|(_, _, why)| *why)
}

/// The hit number of a dump file (`<id>#<n>.json`).
fn hit_n(p: &Path) -> Option<u64> {
    let name = p.file_name()?.to_string_lossy().into_owned();
    let n: String = name
        .rsplit_once('#')?
        .1
        .chars()
        .take_while(char::is_ascii_digit)
        .collect();
    n.parse().ok()
}

/// Every `<fn>:in#n` dump in `state` (or only the lowest n), each with the `<fn>:out#n`
/// of the same call.
fn pairs(state: &Path, f: &str, all: bool) -> Vec<(PathBuf, Option<PathBuf>)> {
    let dirs = vec![state.to_path_buf()];
    let mut ins: Vec<(u64, PathBuf)> = Vec::new();
    if all {
        let prefix = format!("{f}:in#");
        if let Ok(rd) = std::fs::read_dir(state) {
            for e in rd.filter_map(Result::ok) {
                let p = e.path();
                if p.file_name()
                    .is_some_and(|n| n.to_string_lossy().starts_with(&prefix))
                {
                    if let Some(n) = hit_n(&p) {
                        ins.push((n, p));
                    }
                }
            }
        }
        ins.sort();
    } else if let Some(p) = testkit::find_dump(&dirs, &format!("{f}:in")) {
        if let Some(n) = hit_n(&p) {
            ins.push((n, p));
        }
    }
    ins.into_iter()
        .map(|(n, pin)| (pin, testkit::find_dump(&dirs, &format!("{f}:out#{n}"))))
        .collect()
}

#[derive(Default)]
struct Tally {
    pairs: usize,
    clean: usize,
    noop: usize,
    /// pairs listed in [`KNOWN_DIFFS`] that differ (excluded from the gate)
    known: Vec<String>,
    /// pairs listed in [`KNOWN_DIFFS`] that are clean (the list is stale: fails)
    stale: Vec<String>,
    missing: Vec<String>,
    failed: Vec<String>,
    by_path: BTreeMap<String, (usize, String)>,
    /// applyEnemyModifiers pairs whose call keeps an already filled cache (`clearCache`
    /// falsy, `actor.appliedEnemyModifiers` holding keys): the `cache[mod]` hit path
    filled_cache: usize,
}

/// `a.b[3].c` → `a.b[#].c`.
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

fn clip(s: &str) -> String {
    if s.len() > 200 {
        let end = s.char_indices().nth(200).map_or(s.len(), |(i, _)| i);
        format!("{}…", &s[..end])
    } else {
        s.to_owned()
    }
}

impl Tally {
    fn add(&mut self, what: &str, mm: &[Mismatch]) {
        let verbose = std::env::var_os("PFA_VERBOSE").is_some();
        for m in mm {
            let key = format!("{:?} {}", m.kind, generalize(&m.path));
            let e = self.by_path.entry(key).or_insert_with(|| {
                (
                    0,
                    format!(
                        "{what}: {} pob={} ours={}",
                        m.path,
                        m.pob.as_ref().map_or("-".into(), |x| clip(&x.to_string())),
                        m.ours.as_ref().map_or("-".into(), |x| clip(&x.to_string()))
                    ),
                )
            });
            e.0 += 1;
            if verbose {
                eprintln!(
                    "  {what}: {} {:?} pob={:?} ours={:?}",
                    m.path, m.kind, m.pob, m.ours
                );
            }
        }
    }

    fn report(&self, title: &str) {
        eprintln!(
            "{title}: {} pairs, {} with zero mismatches ({} of them no-ops in PoB), {} known diffs, {} missing (not called), {} failed",
            self.pairs,
            self.clean,
            self.noop,
            self.known.len(),
            self.missing.len(),
            self.failed.len()
        );
        if self.filled_cache > 0 {
            eprintln!(
                "  {} of the pairs keep a filled appliedEnemyModifiers cache",
                self.filled_cache
            );
        }
        for f in self.failed.iter().take(10) {
            eprintln!("  failed: {f}");
        }
        for k in &self.known {
            eprintln!("  known: {k}");
        }
        for s in &self.stale {
            eprintln!("  STALE known diff: {s}");
        }
        let mut paths: Vec<_> = self.by_path.iter().collect();
        paths.sort_by(|a, b| b.1 .0.cmp(&a.1 .0));
        for (p, (n, ex)) in paths.iter().take(25) {
            eprintln!("  {n:6} {p}\n         e.g. {ex}");
        }
    }
}

/// Inject `:in`, run the Rust helper, diff against `:out`. Returns (mismatches after the
/// run, whether `:in` already equalled `:out`).
fn run_pair(
    xml: &Path,
    stem: &str,
    f: &str,
    din: &StateDump,
    dout: &StateDump,
) -> Result<(Vec<Mismatch>, bool), String> {
    // untouched injection vs :out (non-vacuity)
    let build = testkit::empty_build();
    let inj0 = pob_oracle::inject::inject(din, &build)?;
    // (no `ret` local: its mismatch is ignored, only the side effects count here)
    let locals0: Vec<(String, Val)> = inj0.locals.into_iter().collect();
    let before = testkit::diff_against(dout, inj0.env.as_ref(), &locals0, stem);
    let noop = before.iter().all(|m| m.path.starts_with("locals.ret"));

    let build = testkit::empty_build();
    let inj = pob_oracle::inject::inject(din, &build)?;
    let locals: Vec<(String, Val)> = inj.locals.into_iter().collect();
    if f == "getCachedOutputValue" {
        if let Some(env) = &inj.env {
            seed_socket_group_list(env, xml);
            replay_build_active_skill(din, dout, env)?;
            if let Some(skill) = locals
                .iter()
                .find(|(k, _)| k == "activeSkill")
                .and_then(|(_, v)| v.obj::<pob_calc::active_skill::ActiveSkill>())
            {
                let uuid = pob_calc::modules::common::cacheSkillUUID(&skill, env);
                if pob_calc::global_cache::cachedData(env.mode, uuid).is_none() {
                    let keys: Vec<String> = pob_calc::global_cache::GlobalCache.with(|g| {
                        g.borrow().cachedData[env.mode.index()]
                            .keys()
                            .map(|k| k.to_string())
                            .collect()
                    });
                    return Err(format!(
                        "the Rust uuid {uuid} is not a GlobalCache key ({keys:?})"
                    ));
                }
            }
        }
    }
    let run = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        pob_calc::modules::calc_perform::actor_a::probe_call(f, &locals)
    }));
    let after = match run {
        Ok(Ok(after)) => after,
        Ok(Err(e)) => return Err(e),
        Err(p) => {
            let msg = p
                .downcast_ref::<pob_lua::LuaError>()
                .map(|e| e.0.clone())
                .or_else(|| p.downcast_ref::<String>().cloned())
                .or_else(|| p.downcast_ref::<&str>().map(|s| (*s).to_owned()))
                .unwrap_or_else(|| "panic".to_owned());
            return Err(format!("run: {msg}"));
        }
    };
    let mut mm = testkit::diff_against(dout, inj.env.as_ref(), &after, stem);
    if f == "getCachedOutputValue" {
        // Replay limit (pob_oracle::replay docs): the callee buildActiveSkill runs a nested
        // initEnv, which re-points build-model gem instances (`gemInstance.displayEffect`,
        // D13) to fresh skill effects of the nested env; the delta replay updates such a
        // table in place, so the identity of those tables (`$ref` links between
        // `env.requirementsTable[i].sourceGem` and the skills' effect lists) differs. Those
        // are the callee's effects, not getCachedOutputValue's: they are counted, not gated.
        let is_ref =
            |v: &Option<serde_json::Value>| v.as_ref().is_some_and(|j| j.get("$ref").is_some());
        let before = mm.len();
        mm.retain(|m| !(is_ref(&m.pob) || is_ref(&m.ours)));
        REPLAY_IDENTITY.with(|c| c.set(c.get() + before - mm.len()));
    }
    Ok((mm, noop))
}

thread_local! {
    /// Mismatches excluded by the getCachedOutputValue replay-identity rule (reported).
    static REPLAY_IDENTITY: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

/// `cacheSkillUUID` (Common.lua:913-917) numbers a skill's group by its index in
/// `build.skillsTab.socketGroupList`. Dumps carry the build only as a `$object` marker and
/// the build loader (BM-A) is a stub, so the injected build has no groups. They are rebuilt
/// from the build XML: one entry per `<Skill>` of the active skill set, in order; an
/// injected group (reached from the player's active skills) takes the slot whose gem
/// `nameSpec` list equals its own, every other slot is an empty placeholder group. A wrong
/// index would show as a missed GlobalCache key (the test reports it).
fn seed_socket_group_list(env: &pob_calc::env::EnvRef, xml: &Path) {
    use pob_calc::model::{SocketGroup, SocketGroupRef};
    let text = std::fs::read_to_string(xml).unwrap_or_default();
    let attr = |s: &str, k: &str| -> Option<String> {
        let p = s.find(&format!("{k}=\""))? + k.len() + 2;
        Some(s[p..p + s[p..].find('"')?].to_owned())
    };
    let skills = text.split("<Skills").nth(1).unwrap_or("");
    let active = attr(skills, "activeSkillSet").unwrap_or_else(|| "1".to_owned());
    let set = skills
        .split("<SkillSet ")
        .skip(1)
        .find(|s| attr(s, "id").as_deref() == Some(active.as_str()))
        .unwrap_or(skills);
    let set = set.split("</SkillSet>").next().unwrap_or(set);
    let xml_groups: Vec<Vec<String>> = set
        .split("<Skill ")
        .skip(1)
        .map(|g| {
            g.split("<Gem ")
                .skip(1)
                .filter_map(|gem| attr(gem, "nameSpec"))
                .collect()
        })
        .collect();
    let mut live: Vec<SocketGroupRef> = Vec::new();
    for skill in env.player.activeSkillList.get().snapshot() {
        if let Some(g) = skill.socketGroup.get() {
            if !live.iter().any(|x| std::rc::Rc::ptr_eq(x, &g)) {
                live.push(g);
            }
        }
    }
    let groups = pob_lua::Shared::new(Vec::new());
    for names in &xml_groups {
        let hit = live.iter().find(|g| {
            let own: Vec<String> = g
                .gemList
                .get()
                .snapshot()
                .iter()
                .map(|gem| gem.nameSpec.get().to_string())
                .collect();
            &own == names
        });
        groups.push(
            hit.cloned()
                .unwrap_or_else(|| std::rc::Rc::new(SocketGroup::default())),
        );
    }
    env.build.skillsTab.socketGroupList.set(groups);
}

/// `getCachedOutputValue` calls `calcs.buildActiveSkill` (CA, still a stub) when the
/// skill's GlobalCache entry is absent (CalcPerform.lua:33-35). The replay of that callee:
/// its effect is the `:in → :out` delta of the call (getCachedOutputValue writes nothing
/// else), applied with `pob_oracle::replay`, i.e. the new `GlobalCache.cachedData[mode][uuid]`
/// entries. Their `.Env` is another env, dumped only as a `$env` marker, so the outputs the
/// function reads from it (`.Env.player.output[name]`) are not in the dump: they are filled
/// from PoB's return values (`ret`, in vararg order) on each new entry's env. What the test
/// then checks is the Rust side's uuid (it must find PoB's key), the lookup and the
/// unpacking, and that nothing else changes.
fn replay_build_active_skill(
    din: &StateDump,
    dout: &StateDump,
    env: &pob_calc::env::EnvRef,
) -> Result<(), String> {
    use pob_calc::env::Mode;
    let mut dumps = BTreeMap::new();
    dumps.insert(din.hit.clone(), din.clone());
    dumps.insert(dout.hit.clone(), dout.clone());
    let replay = pob_oracle::replay::Replay::load("getCachedOutputValue", &dumps);
    let applied = replay.apply_report(&din.hit, env)?;
    if !applied.notes.is_empty() {
        return Err(format!("replay notes: {:?}", applied.notes));
    }
    let entries = |d: &StateDump, mode: &str| -> Vec<String> {
        d.root["globalCache"]["cachedData"][mode]
            .as_object()
            .map(|m| m.keys().cloned().collect())
            .unwrap_or_default()
    };
    let names: Vec<String> = din
        .locals()
        .and_then(|l| l.get("varargs"))
        .and_then(|v| v.get("$seq"))
        .and_then(|s| s.as_array())
        .map(|a| {
            a.iter()
                .filter_map(|x| x.as_str().map(str::to_owned))
                .collect()
        })
        .unwrap_or_default();
    let ret: Vec<serde_json::Value> = dout
        .locals()
        .and_then(|l| l.get("ret"))
        .and_then(|v| v.get("$seq"))
        .and_then(|s| s.as_array())
        .cloned()
        .unwrap_or_default();
    // the skill's entry: `.Name` is its granted effect's name (Common.lua:926)
    let resolved = pob_oracle::schema::resolve_ids(&din.root);
    // follow `{"$ref": "<path>"}` links
    let deref = |v: &serde_json::Value| -> Option<serde_json::Value> {
        match v.get("$ref").and_then(|r| r.as_str()) {
            Some(p) => pob_oracle::schema::get_path(&resolved, p).cloned(),
            None => Some(v.clone()),
        }
    };
    let want_name: Option<String> = resolved["locals"]
        .get("activeSkill")
        .and_then(deref)
        .and_then(|s| s.get("activeEffect").and_then(deref))
        .and_then(|e| e.get("grantedEffect").and_then(deref))
        .and_then(|g| g.get("$data").and_then(|d| d.as_str()).map(str::to_owned))
        .as_deref()
        .and_then(|p| p.strip_prefix("data.skills."))
        .and_then(|id| {
            pob_gamedata::data()
                .skills
                .get(&pob_lua::Name::from(id))
                .map(|ge| ge.name.to_string())
        });
    for (mode, m) in [
        ("MAIN", Mode::MAIN),
        ("CALCS", Mode::CALCS),
        ("CALCULATOR", Mode::CALCULATOR),
    ] {
        let before = entries(din, mode);
        for uuid in entries(dout, mode) {
            let entry_name = dout.root["globalCache"]["cachedData"][mode][&uuid]["Name"].as_str();
            // a new entry (buildActiveSkill ran), or the one the call read (same skill name)
            let read_here = want_name.is_some() && entry_name == want_name.as_deref();
            if before.contains(&uuid) && !read_here {
                continue;
            }
            let Some(entry) =
                pob_calc::global_cache::cachedData(m, pob_lua::Name::from(uuid.as_str()))
            else {
                return Err(format!("replayed GlobalCache entry {mode}/{uuid} missing"));
            };
            let output = entry.Env.player.output.get();
            for (name, v) in names.iter().zip(&ret) {
                if let Some(x) = pob_oracle::schema::decode_num(v) {
                    output.set(name.as_str(), x);
                }
            }
        }
    }
    Ok(())
}

/// An `applyEnemyModifiers(actor, clearCache)` call that keeps the actor's cache
/// (`clearCache` falsy) while it already holds keys: the path where `cache[mod]` hits
/// (CalcPerform.lua:620-627).
fn keeps_filled_cache(din: &StateDump) -> bool {
    let resolved = pob_oracle::schema::resolve_ids(&din.root);
    let locals = &resolved["locals"];
    if locals
        .get("clearCache")
        .is_some_and(|v| !v.is_null() && v.as_bool() != Some(false))
    {
        return false;
    }
    let actor = locals
        .get("actor")
        .and_then(|a| match a.get("$ref").and_then(|r| r.as_str()) {
            Some(p) => pob_oracle::schema::get_path(&resolved, p),
            None => Some(a),
        });
    actor
        .and_then(|a| a.get("appliedEnemyModifiers"))
        .and_then(|c| c.get("$keys"))
        .and_then(|k| k.as_array())
        .is_some_and(|k| !k.is_empty())
}

/// Probe every build with `probe`, run every dumped pair, report per function; true when
/// every pair is clean.
fn run_corpus(
    title: &str,
    builds: &[PathBuf],
    fns: &[&'static str],
    probe: impl Fn(&Path) -> Result<PathBuf, String>,
    all_hits: bool,
) -> bool {
    let mut tallies: BTreeMap<&str, Tally> = BTreeMap::new();
    for xml in builds {
        let stem = stem(xml);
        let state = match probe(xml) {
            Ok(s) => s,
            Err(e) => {
                for f in fns {
                    tallies
                        .entry(f)
                        .or_default()
                        .failed
                        .push(format!("{stem}: {e}"));
                }
                continue;
            }
        };
        for f in fns {
            let t = tallies.entry(f).or_default();
            let ps = pairs(&state, f, all_hits);
            if ps.is_empty() {
                t.missing.push(format!("{stem} {f}:in (not called)"));
            }
            for (pin, pout) in ps {
                let what = format!("{stem} {}", pin.file_name().unwrap().to_string_lossy());
                let Some(pout) = pout else {
                    // PoB raised inside the call (no :out)
                    t.failed.push(format!("{what}: no :out dump"));
                    continue;
                };
                let (Ok(din), Ok(dout)) = (StateDump::load(&pin), StateDump::load(&pout)) else {
                    t.failed.push(format!("{what}: unreadable dump"));
                    continue;
                };
                t.pairs += 1;
                if *f == "applyEnemyModifiers" && keeps_filled_cache(&din) {
                    t.filled_cache += 1;
                }
                let known = known_diff(stem, f);
                match run_pair(xml, stem, f, &din, &dout) {
                    Ok((mm, noop)) => {
                        if mm.is_empty() {
                            t.clean += 1;
                            if noop {
                                t.noop += 1;
                            }
                            if let Some(why) = known {
                                t.stale.push(format!(
                                    "{what}: listed as a known diff ({why}) but clean"
                                ));
                            }
                        } else if let Some(why) = known {
                            t.known
                                .push(format!("{what}: {} mismatches ({why})", mm.len()));
                            continue;
                        }
                        t.add(&what, &mm);
                    }
                    Err(e) => t.failed.push(format!("{what}: {e}")),
                }
            }
        }
    }
    let mut ok = true;
    let mut total = 0;
    eprintln!(
        "== {title}: {} builds ({} getCachedOutputValue `$ref` identity mismatches of the replayed buildActiveSkill excluded)",
        builds.len(),
        REPLAY_IDENTITY.with(|c| c.replace(0))
    );
    for f in fns {
        let t = tallies.entry(f).or_default();
        t.report(f);
        total += t.pairs;
        ok &= t.clean + t.known.len() == t.pairs && t.failed.is_empty() && t.stale.is_empty();
    }
    assert!(total > 0, "no function-probe dumps");
    let filled = tallies
        .get("applyEnemyModifiers")
        .map_or(0, |t| t.filled_cache);
    FILLED_CACHE.with(|c| c.set(filled));
    ok
}

thread_local! {
    /// [`Tally::filled_cache`] of applyEnemyModifiers in the last [`run_corpus`].
    static FILLED_CACHE: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

/// The first call of each helper in every build's main env (all57 + unit corpus).
#[cfg(feature = "luajit")]
#[test]
fn pfa_function_probes_all57() {
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
    let ok = run_corpus("main env, first call", &builds, FNS, probe_dumps, false);
    assert!(ok, "some PF-A function probes differ from PoB");
}

/// Builds of the hit sample: a minion build (039), six curses (041), mirage/curses
/// (Mirage Archer Toxic Rain), plus the unit corpus. `PFA_HIT_BUILDS` overrides.
const HIT_BUILDS: &[&str] = &[
    "allflame_exp_039",
    "allflame_exp_041",
    "upstream_Mirage_Archer_Toxic_Rain",
    "pfa_cached_outputs",
];

/// Calls `#1..#12` of each helper in any env (other modes, buildActiveSkill /
/// calcFullDPS envs, minion actors, later calls with `addAura` / without `clearCache`).
#[cfg(feature = "luajit")]
#[test]
fn pfa_function_probes_hit_sample() {
    if !testkit::init_default_data() {
        return skip("no data dump (data/dump)");
    }
    let Some(parity) = find_parity_dir() else {
        return skip("no .parity directory");
    };
    if pob_oracle::oracle::find_pob_src().is_none() {
        return skip("no PoB checkout");
    }
    let want: Vec<String> = std::env::var("PFA_HIT_BUILDS").map_or_else(
        |_| HIT_BUILDS.iter().map(|s| (*s).to_owned()).collect(),
        |s| s.split(',').map(|x| x.trim().to_owned()).collect(),
    );
    let builds: Vec<PathBuf> = corpus(&parity)
        .into_iter()
        .filter(|p| want.iter().any(|w| w == stem(p)))
        .collect();
    if builds.is_empty() {
        return skip("none of the hit-sample builds found");
    }
    let ok = run_corpus(
        "hit sample #1..#12, any env",
        &builds,
        FNS,
        probe_hit_sample,
        true,
    );
    assert!(ok, "some PF-A function probes differ from PoB");
}

/// Later `applyEnemyModifiers` calls that keep a filled cache ([`LATE_BUILDS`],
/// [`LATE_HITS`]); fails when none of the probed pairs has one (non-vacuity).
#[cfg(feature = "luajit")]
#[test]
fn pfa_apply_enemy_modifiers_filled_cache() {
    if !testkit::init_default_data() {
        return skip("no data dump (data/dump)");
    }
    let Some(parity) = find_parity_dir() else {
        return skip("no .parity directory");
    };
    if pob_oracle::oracle::find_pob_src().is_none() {
        return skip("no PoB checkout");
    }
    let builds: Vec<PathBuf> = corpus(&parity)
        .into_iter()
        .filter(|p| LATE_BUILDS.contains(&stem(p)))
        .collect();
    if builds.is_empty() {
        return skip("none of the late-hit builds found");
    }
    let ok = run_corpus(
        "applyEnemyModifiers late hits, filled cache",
        &builds,
        &["applyEnemyModifiers"],
        probe_late_hits,
        true,
    );
    assert!(ok, "some applyEnemyModifiers late hits differ from PoB");
    assert!(
        FILLED_CACHE.with(std::cell::Cell::get) > 0,
        "no applyEnemyModifiers call with a filled cache was probed"
    );
}

/// The probes are placed where the unit's ranges begin (guards stages.toml against drift).
#[cfg(feature = "luajit")]
#[test]
fn pfa_stages_file_patches() {
    let Some(src) = pob_oracle::oracle::find_pob_src() else {
        return skip("no PoB checkout");
    };
    let stages =
        pob_oracle::patcher::StagesFile::load(&stages_file()).expect("corpus/PF-A/stages.toml");
    assert_eq!(stages.function.len(), 6);
    let plan = pob_oracle::patcher::plan(&src, &stages).expect("plan");
    pob_oracle::patcher::validate(&plan).expect("patched files load");
}
