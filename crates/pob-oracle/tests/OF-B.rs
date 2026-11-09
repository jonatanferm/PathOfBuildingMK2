//! Unit OF-B acceptance (calc doc §11 row OF-B, §9.4 L4): CalcOffence.lua 1037-1691
//! (`calc_offence::skill_types::seg_skillTypeStats`).
//!
//! **L4.** For every corpus build with dumps, the pob-state/1 dump PoB wrote at the line probe
//! `offence:1037` is injected into the Rust graph (`pob_oracle::inject`), the Rust
//! segment runs (`pob_calc::probe::run_segment`), and the exported state (env + the dumped
//! locals) is diffed against PoB's `offence:1692` dump of the same `offence` call
//! (`pob_oracle::testkit::diff_against`, §9.2 tolerance, nondet record honoured). Every hit
//! of the probe pair in the build's oracle process is a case (`offence:1037#n` →
//! `offence:1692#n`: every `calcs.offence` call of every env PoB built while loading, so
//! skills of other socket groups and minion skills are covered too); a hit whose in/out
//! dumps equal an earlier hit's is counted once.
//!
//! **Callees.** The segment calls OF-A's `runSkillFunc` (the `preSkillTypeFunc` hook),
//! `calcAreaOfEffect`, `calcAreaMod`, `calcRadius`, `calcRadiusBreakpoints`,
//! `calcSkillCooldown`, `calcSkillDuration`, `calcWarcryCastTime` and `calcTotemLife`. While
//! OF-A is not merged they are `unported!` stubs returning defaults, and ORC-X's replay
//! cannot stand in for them mid-segment (it applies a recorded delta between two dumps; the
//! Rust segment has no hook where a callee's delta or return value could be applied;
//! CCR-0044). So each run records which unported sites it hit (`stub_report`), and a
//! mismatch is **attributed** to a hit stub only when its path is one the stub's Lua writes,
//! or one an OF-B statement writes from the stub's return value (the table [`ATTRIBUTION`],
//! each entry citing the Lua lines). Everything else must match exactly. A run that raises
//! is attributed only when a stub was hit before it (a default return value fed a later
//! statement). Once OF-A lands, its sites are no longer hit and the same test is strict.
//! A hit whose active effect has a `preSkillTypeFunc` hook (Rage Vortex, Bane, Dark
//! Ritual) and differs outside those paths is reported separately: the hook can add mods
//! anywhere. So is a differing hit whose dump references another env's objects
//! (`{"$foreign": …}`: a mirage or trigger env's skill copied from the main env's, whose
//! `baseSkillModList.parent` is the main env's modDB), which injection can only stub out.
//! Both are excluded from the gate's denominator and listed by name.
//!
//! **Corpus.** all57 (`.parity/builds`) plus the unit corpus `corpus/OF-B/*.xml` (`derive.py`:
//! slice5 builds with the main group swapped to two mines (one an aura), a brand, a corpse
//! skill and a trap with a cooldown — branches no all57 main skill reaches).
//!
//! **Gate** (calc doc §9.4 L4): every unit-corpus hit passes and ≥ 98% of all distinct probed
//! hits pass (no unattributed mismatch);
//! with `OFB_STRICT=1` every hit must also have zero attributed mismatches (the gate once
//! OF-A is merged).
//!
//! Inputs: `.parity/probe-cache/*/<build-key>/offence:{1037,1692}#n.json.zst` (fill with
//! `scripts/port/oracle.sh run --build B.xml --stage 'offence:1037#*,offence:1692#*'`),
//! `data/dump`. Knobs: `OFB_BUILDS=stem1,stem2`, `OFB_VERBOSE=1` (every mismatch).

use std::collections::{BTreeMap, HashSet};
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};

use pob_calc::lua_table::Val;
use pob_oracle::diff::{Mismatch, MismatchKind};
use pob_oracle::schema::StateDump;
use pob_oracle::testkit;

const STAGE_IN: &str = "offence:1037";
const STAGE_OUT: &str = "offence:1692";

/// OF-A's unported callees: stub site → (callee, the paths whose mismatch the stub explains).
/// A path pattern is matched against the diff path with its `env.<actor>.` / `locals.` prefix
/// removed: `output.X`, `breakdown.X`, `skillData.X`, `skillFlags.X`; a trailing `*` matches
/// any suffix of the key (including nested breakdown rows).
const ATTRIBUTION: &[(&str, &str, &[&str])] = &[
    // calcAreaOfEffect writes these (CalcOffence.lua 376-492, setMoltenStrike… 235-294);
    // MineAuraRadius (1404-1410) is computed from output.AreaOfEffectMod.
    (
        "Modules/CalcOffence.lua:374-493",
        "calcAreaOfEffect",
        &[
            "output.AreaOfEffect*",
            "breakdown.AreaOfEffect*",
            "output.SpeedModTertiary",
            "skillData.radius",
            "skillFlags.area",
            "output.MineAuraRadius*",
            "breakdown.MineAuraRadius*",
        ],
    ),
    // calcAreaMod / calcRadius / calcRadiusBreakpoints feed the trap trigger (1311-1318), mine
    // detonation (1395-1402) and mine aura (1403-1411) radii and their breakdowns.
    (
        "Modules/CalcOffence.lua:160-166",
        "calcAreaMod",
        &[
            "output.TrapTriggerRadius*",
            "breakdown.TrapTriggerRadius*",
            "output.MineDetonationRadius*",
            "breakdown.MineDetonationRadius*",
            "output.MineAuraRadius*",
            "breakdown.MineAuraRadius*",
        ],
    ),
    (
        "Modules/CalcOffence.lua:173-178",
        "calcRadius",
        &[
            "output.TrapTriggerRadius*",
            "breakdown.TrapTriggerRadius*",
            "output.MineDetonationRadius*",
            "breakdown.MineDetonationRadius*",
            "output.MineAuraRadius*",
            "breakdown.MineAuraRadius*",
        ],
    ),
    (
        "Modules/CalcOffence.lua:198-224",
        "calcRadiusBreakpoints",
        &[
            "breakdown.TrapTriggerRadius*",
            "breakdown.MineDetonationRadius*",
            "breakdown.MineAuraRadius*",
        ],
    ),
    // calcSkillCooldown: output.TrapCooldown (1299-1300) / output.Cooldown and its breakdown
    // (1320-1331); the uptimes (1662-1688) divide by output.Cooldown.
    (
        "Modules/CalcOffence.lua:296-309",
        "calcSkillCooldown",
        &[
            "output.TrapCooldown",
            "breakdown.TrapCooldown*",
            "output.Cooldown",
            "breakdown.Cooldown*",
            "output.*Uptime",
            "breakdown.*Uptime*",
        ],
    ),
    // calcWarcryCastTime: output.WarcryCastTime (1469).
    (
        "Modules/CalcOffence.lua:311-320",
        "calcWarcryCastTime",
        &["output.WarcryCastTime"],
    ),
    // calcSkillDuration: the warcry uptime in output.WarcryEffectMod (1220-1238).
    (
        "Modules/CalcOffence.lua:322-335",
        "calcSkillDuration",
        &["output.WarcryEffectMod", "breakdown.WarcryEffectMod*"],
    ),
    // calcs.calcTotemLife: output.TotemLife / TotemLifeMod and breakdown.TotemLife (1440-1452).
    (
        "Modules/CalcOffence.lua:338-342",
        "calcTotemLife",
        &[
            "output.TotemLife",
            "output.TotemLifeMod",
            "breakdown.TotemLife*",
        ],
    ),
    // runSkillFunc: without a hook on the active effect it does nothing (509-514).
    ("Modules/CalcOffence.lua:509-514", "runSkillFunc", &[]),
];

fn xml_files(dir: &Path) -> Vec<PathBuf> {
    let mut v: Vec<PathBuf> = std::fs::read_dir(dir)
        .map(|r| {
            r.filter_map(Result::ok)
                .map(|e| e.path())
                .filter(|p| p.extension().is_some_and(|x| x == "xml"))
                .collect()
        })
        .unwrap_or_default();
    v.sort();
    v
}

/// all57 (`.parity/builds`) plus the unit corpus (`corpus/OF-B/*.xml`, derive.py).
fn corpus() -> Vec<PathBuf> {
    let Some(parity) =
        pob_testpaths::require(pob_testpaths::parity_dir(), ".parity (build corpus)")
    else {
        return Vec::new();
    };
    let mut v = xml_files(&parity.join("builds"));
    v.extend(xml_files(
        &Path::new(env!("CARGO_MANIFEST_DIR")).join("corpus/OF-B"),
    ));
    if let Ok(list) = std::env::var("OFB_BUILDS") {
        let want: Vec<&str> = list.split(',').map(str::trim).collect();
        v.retain(|p| want.contains(&stem(p)));
    }
    v
}

fn stem(p: &Path) -> &str {
    p.file_stem().and_then(|s| s.to_str()).unwrap_or_default()
}

/// The path relative to its owner: `env.player.output.X` → `output.X`,
/// `locals.skillData.radius` → `skillData.radius`.
fn rel_path(path: &str) -> &str {
    for owner in ["output.", "breakdown.", "skillData.", "skillFlags."] {
        if let Some(i) = path.find(&format!(".{owner}")) {
            return &path[i + 1..];
        }
        if path.starts_with(owner) {
            return path;
        }
    }
    path
}

/// `pat` (`output.AreaOfEffect*`, `output.*Uptime`, `breakdown.*Uptime*`) against `rel`.
fn path_matches(pat: &str, rel: &str) -> bool {
    let (owner, key_pat) = pat.split_once('.').unwrap_or((pat, ""));
    let Some(rest) = rel.strip_prefix(owner).and_then(|r| r.strip_prefix('.')) else {
        return false;
    };
    // the key is up to the next `.` / `[` (a nested breakdown row stays with its key)
    let end = rest.find(['.', '[']).unwrap_or(rest.len());
    let (key, tail) = (&rest[..end], &rest[end..]);
    let glob_tail = key_pat.ends_with('*');
    let kp = key_pat.trim_end_matches('*');
    let key_ok = if let Some(suffix) = kp.strip_prefix('*') {
        key.ends_with(suffix)
    } else if glob_tail {
        key.starts_with(kp)
    } else {
        key == kp
    };
    key_ok && (tail.is_empty() || glob_tail)
}

/// The callees (by name) whose stub explains `m`, among the `hit` stub sites.
fn attributed_to(m: &Mismatch, hit: &[&str]) -> Option<&'static str> {
    let rel = rel_path(&m.path);
    ATTRIBUTION
        .iter()
        .filter(|(site, _, _)| hit.contains(site))
        .find(|(_, _, pats)| pats.iter().any(|p| path_matches(p, rel)))
        .map(|(_, name, _)| *name)
}

#[derive(Default)]
struct Tally {
    builds: usize,
    duplicates: usize,
    probed: usize,
    no_dumps: Vec<String>,
    pass_strict: Vec<String>,
    pass_attributed: Vec<(String, usize, Vec<String>)>,
    hook: Vec<(String, usize)>,
    foreign: Vec<(String, Vec<String>)>,
    fail: Vec<(String, Vec<String>)>,
    blocked: Vec<(String, String, Vec<String>)>,
    /// paths the segment changed in PoB (din → dout), summed: the non-vacuity measure.
    changed_paths: usize,
    attributed_by: BTreeMap<&'static str, usize>,
}

fn stub_counts() -> BTreeMap<&'static str, u64> {
    pob_calc::stub::stub_report().into_iter().collect()
}

/// The stub sites hit between two `stub_counts` snapshots.
fn hit_between(
    a: &BTreeMap<&'static str, u64>,
    b: &BTreeMap<&'static str, u64>,
) -> Vec<&'static str> {
    b.iter()
        .filter(|(k, v)| a.get(*k).copied().unwrap_or(0) < **v)
        .map(|(k, _)| *k)
        .collect()
}

fn fmt_mm(m: &Mismatch) -> String {
    let clip = |v: &Option<serde_json::Value>| {
        let s = v.as_ref().map_or("-".to_owned(), ToString::to_string);
        if s.len() > 160 {
            format!(
                "{}…",
                &s[..s.char_indices().nth(160).map_or(s.len(), |(i, _)| i)]
            )
        } else {
            s
        }
    };
    format!(
        "{} {:?}: pob={} ours={}",
        m.path,
        m.kind,
        clip(&m.pob),
        clip(&m.ours)
    )
}

/// Does the injected active skill's active effect carry a `preSkillTypeFunc` hook?
fn has_pre_skill_type_hook(locals: &[(String, Val)]) -> bool {
    locals
        .iter()
        .find(|(k, _)| k == "activeSkill")
        .and_then(|(_, v)| v.obj::<pob_calc::active_skill::ActiveSkill>())
        .is_some_and(|s| {
            s.activeEffect
                .get()
                .grantedEffect
                .get()
                .preSkillTypeFunc
                .is_some()
        })
}

/// Does the parent chain of the dumped `skillModList` end in another env's object
/// (`{"$foreign": …}`)? `copyActiveSkill` (CalcActiveSkill.lua:184) chains a mirage/trigger
/// env's skill to the source env's mod stores; the dump of the nested env only names them.
fn foreign_skill_mod_list(root: &serde_json::Value) -> bool {
    use serde_json::Value as J;
    fn index<'a>(v: &'a J, ids: &mut BTreeMap<u64, &'a J>) {
        match v {
            J::Object(m) => {
                if let Some(id) = m.get("$id").and_then(J::as_u64) {
                    ids.insert(id, v);
                }
                m.values().for_each(|x| index(x, ids));
            }
            J::Array(a) => a.iter().for_each(|x| index(x, ids)),
            _ => {}
        }
    }
    let mut ids = BTreeMap::new();
    index(root, &mut ids);
    let deref = |v: &J| -> Option<J> {
        let mut v = v.clone();
        for _ in 0..64 {
            match v.get("$ref").and_then(J::as_u64) {
                Some(r) => v = (*ids.get(&r)?).clone(),
                None => return Some(v),
            }
        }
        None
    };
    let mut cur = root
        .get("locals")
        .and_then(|l| l.get("skillModList"))
        .and_then(&deref);
    for _ in 0..16 {
        let Some(v) = cur else { return false };
        if v.get("$foreign").is_some() {
            return true;
        }
        cur = v.get("parent").and_then(&deref);
    }
    false
}

/// Every `(n, offence:1037#n, offence:1692#n)` pair of dumps of a build (first directory wins).
fn hit_pairs(dirs: &[PathBuf]) -> Vec<(u64, PathBuf, PathBuf)> {
    let mut found: BTreeMap<u64, (PathBuf, PathBuf)> = BTreeMap::new();
    for d in dirs {
        let Ok(rd) = std::fs::read_dir(d) else {
            continue;
        };
        for e in rd.filter_map(Result::ok) {
            let name = e.file_name().to_string_lossy().into_owned();
            let Some(rest) = name.strip_prefix(&format!("{STAGE_IN}#")) else {
                continue;
            };
            let digits: String = rest.chars().take_while(char::is_ascii_digit).collect();
            let Ok(n) = digits.parse::<u64>() else {
                continue;
            };
            if found.contains_key(&n) {
                continue;
            }
            if let Some(pout) = testkit::find_dump(dirs, &format!("{STAGE_OUT}#{n}")) {
                found.insert(n, (e.path(), pout));
            }
        }
    }
    found.into_iter().map(|(n, (a, b))| (n, a, b)).collect()
}

fn run_build(xml: &Path, t: &mut Tally, seen: &mut HashSet<u64>, verbose: bool) {
    let stem = stem(xml).to_owned();
    let dirs = testkit::dump_dirs(&stem, Some(xml));
    let pairs = hit_pairs(&dirs);
    if pairs.is_empty() {
        t.no_dumps.push(stem);
        return;
    }
    t.builds += 1;
    let model = testkit::load_build(xml).unwrap_or_else(testkit::empty_build);
    for (_, pin, pout) in pairs {
        run_hit(&stem, &model, &pin, &pout, t, seen, verbose);
    }
}

fn run_hit(
    build: &str,
    model: &pob_calc::model::BuildRef,
    pin: &Path,
    pout: &Path,
    t: &mut Tally,
    seen: &mut HashSet<u64>,
    verbose: bool,
) {
    let din = StateDump::load(pin).unwrap_or_else(|e| panic!("{}: {e}", pin.display()));
    let dout = StateDump::load(pout).unwrap_or_else(|e| panic!("{}: {e}", pout.display()));
    // the two line probes must belong to the same `offence` call
    assert_eq!(
        din.stack, dout.stack,
        "{build}: {} and {} are in different calls",
        din.hit, dout.hit
    );
    assert_eq!(din.env_id, dout.env_id, "{build}: dumps of different envs");
    let din_text = din.root.to_string();
    // the injected state lacks what the dump only names: another env's object (a mirage or
    // trigger env's skill copied from the main env's; `{"$foreign": …}` injects as an empty
    // object, pob-state/1, ORC-X)
    let foreign = foreign_skill_mod_list(&din.root);
    // a hit whose in/out states equal an earlier hit's (the same skill in a repeated
    // buildOutput env) is the same test case: count it once
    {
        let mut h = std::collections::hash_map::DefaultHasher::new();
        din_text.hash(&mut h);
        dout.root.to_string().hash(&mut h);
        if !seen.insert(h.finish()) {
            t.duplicates += 1;
            return;
        }
    }
    let stem = format!("{build}/{}", din.hit);
    t.probed += 1;
    t.changed_paths += pob_oracle::diff::diff_trees(
        &pob_oracle::schema::resolve_ids(&din.root),
        &pob_oracle::schema::resolve_ids(&dout.root),
        &pob_oracle::diff::DiffOptions::strict(),
    )
    .len();

    let inj = pob_oracle::inject::inject(&din, model)
        .unwrap_or_else(|e| panic!("{stem}: inject {}: {e}", din.hit));
    let locals: Vec<(String, Val)> = inj.locals.into_iter().collect();
    let hook = has_pre_skill_type_hook(&locals);

    let before = stub_counts();
    let run = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        pob_calc::probe::run_segment(&din.probe, &locals)
    }));
    let after_counts = stub_counts();
    let hit = hit_between(&before, &after_counts);
    let hit_names: Vec<String> = hit
        .iter()
        .map(|s| {
            ATTRIBUTION
                .iter()
                .find(|(site, _, _)| site == s)
                .map_or_else(|| (*s).to_owned(), |(_, n, _)| (*n).to_owned())
        })
        .collect();
    let after = match run {
        Ok(Ok((_flow, after))) => after,
        Ok(Err(e)) => panic!("{stem}: run_segment: {e}"),
        Err(p) => {
            let msg = p
                .downcast_ref::<pob_lua::LuaError>()
                .map(|e| e.0.clone())
                .or_else(|| p.downcast_ref::<String>().cloned())
                .or_else(|| p.downcast_ref::<&str>().map(|s| (*s).to_owned()))
                .unwrap_or_else(|| "panic".to_owned());
            // a raise after a stub returned its default is the stub's (see module docs)
            let explained = hit.iter().any(|s| {
                ATTRIBUTION
                    .iter()
                    .any(|(site, name, _)| site == s && *name != "runSkillFunc")
            });
            if explained || hook || foreign {
                let why = if explained {
                    ""
                } else if hook {
                    " [hook]"
                } else {
                    " [$foreign]"
                };
                t.blocked.push((format!("{stem}{why}"), msg, hit_names));
            } else {
                t.fail.push((stem, vec![format!("raised: {msg}")]));
            }
            return;
        }
    };
    let env = after
        .iter()
        .find(|(k, _)| k == "env")
        .and_then(|(_, v)| v.obj::<pob_calc::env::Env>())
        .or(inj.env);
    let mm: Vec<Mismatch> = testkit::diff_against(&dout, env.as_ref(), &after, &stem)
        .into_iter()
        // mod order inside a store is reported, not gated (calc doc §9.2: LUA-ORDER)
        .filter(|m| m.kind != MismatchKind::LuaOrder)
        .collect();
    let unexplained: Vec<&Mismatch> = mm
        .iter()
        .filter(|m| attributed_to(m, &hit).is_none())
        .collect();
    if verbose {
        for m in &mm {
            eprintln!(
                "  {stem} [{}] {}",
                attributed_to(m, &hit).unwrap_or("UNATTRIBUTED"),
                fmt_mm(m)
            );
        }
    }
    if !unexplained.is_empty() && hook {
        t.hook.push((stem, unexplained.len()));
    } else if !unexplained.is_empty() && foreign {
        t.foreign
            .push((stem, unexplained.iter().map(|m| fmt_mm(m)).collect()));
    } else if !unexplained.is_empty() {
        t.fail
            .push((stem, unexplained.iter().map(|m| fmt_mm(m)).collect()));
    } else if mm.is_empty() {
        t.pass_strict.push(stem);
    } else {
        for m in &mm {
            if let Some(n) = attributed_to(m, &hit) {
                *t.attributed_by.entry(n).or_default() += 1;
            }
        }
        t.pass_attributed.push((stem, mm.len(), hit_names));
    }
}

/// L4 `offence:1037` → `offence:1692` over the corpus.
#[test]
fn ofb_l4_skill_type_stats_all57() {
    let Some(_) = pob_testpaths::require(pob_testpaths::dump_dir(), "data/dump (pob-dump)") else {
        return;
    };
    assert!(testkit::init_default_data(), "data/dump did not load");
    let builds = corpus();
    if builds.is_empty() {
        return; // POB_ALLOW_SKIP=1 (corpus() already reported)
    }
    let verbose = std::env::var_os("OFB_VERBOSE").is_some();
    let strict = std::env::var_os("OFB_STRICT").is_some_and(|v| v == "1");
    let mut t = Tally::default();
    let mut seen = HashSet::new();
    for xml in &builds {
        run_build(xml, &mut t, &mut seen, verbose);
    }

    eprintln!(
        "OF-B L4 {STAGE_IN} -> {STAGE_OUT}: {} builds, {} with dumps; {} distinct hits \
         ({} duplicate hits skipped)",
        builds.len(),
        t.builds,
        t.probed,
        t.duplicates
    );
    eprintln!(
        "  PoB's segment changed {} state paths in total (non-vacuity)",
        t.changed_paths
    );
    eprintln!("  pass (exact):               {}", t.pass_strict.len());
    eprintln!(
        "  pass (only OF-A-stub paths differ): {} ({} mismatches: {:?})",
        t.pass_attributed.len(),
        t.pass_attributed.iter().map(|(_, n, _)| n).sum::<usize>(),
        t.attributed_by
    );
    for (s, n, hits) in &t.pass_attributed {
        eprintln!("    {s}: {n} (stubs hit: {})", hits.join(", "));
    }
    for (s, msg, hits) in &t.blocked {
        eprintln!(
            "  blocked by an OF-A stub: {s}: {msg} (stubs hit: {})",
            hits.join(", ")
        );
    }
    for (s, n) in &t.hook {
        eprintln!("  preSkillTypeFunc hook (not replayable): {s}: {n} unattributed mismatches");
    }
    for (s, lines) in &t.foreign {
        eprintln!(
            "  input references another env ($foreign, not injectable): {s}: {} unattributed",
            lines.len()
        );
        for l in lines.iter().take(if verbose { usize::MAX } else { 3 }) {
            eprintln!("      {l}");
        }
    }
    for (s, lines) in &t.fail {
        eprintln!("  FAIL {s}: {} unattributed", lines.len());
        for l in lines.iter().take(if verbose { usize::MAX } else { 12 }) {
            eprintln!("      {l}");
        }
    }
    if !t.no_dumps.is_empty() {
        eprintln!(
            "  no {STAGE_IN}/{STAGE_OUT} dumps ({}): {}",
            t.no_dumps.len(),
            t.no_dumps.join(", ")
        );
    }

    let probed = t.probed;
    let Some(_) = pob_testpaths::require(
        (probed > 0).then_some(()),
        "offence:1037/1692 dumps (scripts/port/oracle.sh run --build B.xml --stage 'offence:1037#*,offence:1692#*')",
    ) else {
        return;
    };
    assert!(
        t.changed_paths > 0,
        "vacuous: PoB's dumps show no change by the segment"
    );
    let passed = t.pass_strict.len() + t.pass_attributed.len();
    let gated = probed - t.hook.len() - t.foreign.len() - t.blocked.len();
    // ≥ 98% of the gated hits (calc doc §9.4 L4)
    assert!(
        passed as f64 >= 0.98 * gated as f64,
        "{passed} of {gated} hits pass ({} fail)",
        t.fail.len()
    );
    // the unit corpus passes 100% (calc doc §9.4 L4)
    let unit_fail = t.fail.iter().filter(|(s, _)| s.starts_with("ofb_")).count();
    assert_eq!(
        unit_fail, 0,
        "unit corpus (corpus/OF-B): {unit_fail} hits fail"
    );
    if strict {
        assert!(
            t.pass_attributed.is_empty() && t.blocked.is_empty() && t.hook.is_empty(),
            "OFB_STRICT=1: {} hits still differ in OF-A-stub paths, {} blocked, {} hook hits",
            t.pass_attributed.len(),
            t.blocked.len(),
            t.hook.len()
        );
        assert!(
            t.foreign.is_empty(),
            "OFB_STRICT=1: {} hits differ on an un-injectable ($foreign) input",
            t.foreign.len()
        );
        assert_eq!(t.fail.len(), 0, "OFB_STRICT=1: unattributed mismatches");
    }
}

/// The attribution matcher itself (it decides what the L4 gate ignores).
#[test]
fn ofb_attribution_patterns() {
    assert!(path_matches(
        "output.AreaOfEffect*",
        "output.AreaOfEffectRadiusMetres"
    ));
    assert!(!path_matches(
        "output.AreaOfEffect*",
        "output.ProjectileCount"
    ));
    assert!(path_matches("output.*Uptime", "output.DurationUptime"));
    assert!(!path_matches("output.*Uptime", "output.DurationUptime.x"));
    assert!(path_matches(
        "breakdown.*Uptime*",
        "breakdown.DurationUptime[1]"
    ));
    assert!(path_matches("output.Cooldown", "output.Cooldown"));
    assert!(!path_matches("output.Cooldown", "output.CooldownX"));
    assert!(!path_matches("output.Cooldown", "breakdown.Cooldown"));
    assert!(path_matches(
        "breakdown.TotemLife*",
        "breakdown.TotemLife[2]"
    ));
    assert!(!path_matches("output.TotemLife", "output.TotemLifeMod2"));
    assert_eq!(rel_path("env.player.output.Cooldown"), "output.Cooldown");
    assert_eq!(
        rel_path("env.player.mainSkill.skillData.radius"),
        "skillData.radius"
    );
    assert_eq!(rel_path("locals.skillFlags.area"), "skillFlags.area");
    // nothing is attributed when no stub was hit
    let m = Mismatch {
        path: "env.player.output.Cooldown".into(),
        kind: MismatchKind::Value,
        pob: None,
        ours: None,
        writer: None,
    };
    assert_eq!(attributed_to(&m, &[]), None);
    assert_eq!(
        attributed_to(&m, &["Modules/CalcOffence.lua:296-309"]),
        Some("calcSkillCooldown")
    );
    assert_eq!(
        attributed_to(&m, &["Modules/CalcOffence.lua:374-493"]),
        None
    );
}
