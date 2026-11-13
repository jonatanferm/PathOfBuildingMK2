//! Unit INT-3 acceptance (calc doc §11.1 row INT-3, breakdown parity): what the Calcs tab reads
//! after a build load, end to end against PoB (L6 shape; the build model from the XML, then
//! `CalcsTab:BuildOutput` as PoB's load runs it, as `ca_l6_corpus` does).
//!
//! * `int3_breakdown_corpus`: `calcsTab.calcsEnv.player.breakdown` (and the minion's) against
//!   PoB's, structurally: every breakdown entry (a top-level key, and each key of the `MainHand` /
//!   `OffHand` namespaces, CalcBreakdownControl.lua:150-155) as the pob-state/1 encoding
//!   (state_dump.lua on PoB's side, `pob_calc::probe::export_root` on ours), with identities
//!   resolved, items reduced to `{"$item": name}`, mod stores to `{"$store": class}`, actors and
//!   skills to markers, numbers within the §9.2 tolerance. An entry is **equal** when the two trees
//!   are equal after sorting the lists the breakdown control itself sorts before showing them
//!   (`rowList` when the entry has a `source`, 181-190; `slots`, 263-265; `modList` rows, 337-345):
//!   their order on PoB's side is PoB's per-process `pairs` order where it is not fixed (calc doc
//!   D8). It is **exact** when it is also equal in order.
//! * `int3_tabulate_corpus`: every distinct mod query of Modules/CalcSections.lua that
//!   CalcBreakdownControl:AddModSection runs (`modStore:Tabulate(modType, cfg, modName…)`,
//!   CalcBreakdownControl.lua:303-318), on `calcsEnv`'s player and minion, plus the per-source
//!   `Combine` totals of its source tooltips (348-381): rows (value + mod) and totals against
//!   PoB's (`corpus/INT-3/int3.lua`). A query that raises in PoB must raise in Rust too.
//!
//! Accepted, and counted apart (docs/port/parity/INT-3-triage.md §3): `%d` of NaN / ±inf on the
//! ARM64 oracle (CCR-0001; known_diffs `DF-E-1`, `DF-G-1`), the presence of a closure in a
//! breakdown row ([`Accepted::closure`]), the `MaximumHitTaken` rows of `pairs(takenDamages)`,
//! Tabulate row order (LUA-ORDER), and PoB's own per-process choices ([`POB_NONDET_NAMES`],
//! [`nondet_row`]; known_diffs `INT-3-1`, `INT-3-2`, `PF-C-1`).
//!
//! Gate (INT-3): no Rust error, every breakdown entry equal and every Tabulate query equal, on
//! any corpus subset; on the full corpus the compared totals are pinned
//! (docs/port/parity/INT-3-triage.md).
//!
//! PoB runs (`int3.json`, `calcs.json`: the calcsEnv dump, `hits.json`) are cached in the shared,
//! content-keyed oracle cache (`testkit::oracle_dir`, CK1; `POB_QUERY_CACHE=off|refresh|verify`
//! as for the other users). Knobs: `INT3_BUILDS=stem1,stem2`, `INT3_LIMIT=n`,
//! `INT3_VERBOSE=1`, `INT3_CORPUS_DIR=dir` (the corpus from `dir/*.xml`, no pins),
//! `INT3_DIFFS=file` (appends every difference, accepted difference and order-only entry as TSV
//! `stem test actor key path pob ours`), `INT3_STRICT=1` (accept none of PoB's nondeterministic
//! choices).
//! A missing input skips with a message; with `POB_ORACLE_REQUIRE=1` (or `CI`) it fails instead.
//! Owner: INT-3.
#![allow(
    non_snake_case,
    clippy::many_single_char_names,
    clippy::format_push_string
)]

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::{Arc, Mutex};

use pob_calc::modules::calcs::{buildOutput, getMiscCalculator};
use pob_calc::modules::common::wipeGlobalCache;
use pob_calc::prelude::*;
use pob_gamedata::timeless_lut as lut;
use pob_oracle::diff::{diff_trees, tree_eq, DiffOptions, REL_TOL};
use pob_oracle::oracle::{find_parity_dir, find_pob_src, find_up};
use serde_json::{Map, Value as J};

static SERIAL: Mutex<()> = Mutex::new(());

const INT3_LUA: &str = include_str!("../corpus/INT-3/int3.lua");

fn skip(why: &str) {
    let required = std::env::var_os("POB_ORACLE_REQUIRE").is_some_and(|v| v == "1")
        || std::env::var_os("CI").is_some_and(|v| !v.is_empty() && v != "false" && v != "0");
    assert!(!required, "required input missing: {why}");
    eprintln!("SKIPPED: {why}");
}

fn verbose() -> bool {
    std::env::var_os("INT3_VERBOSE").is_some()
}

fn stem(p: &Path) -> &str {
    p.file_stem().and_then(|s| s.to_str()).unwrap_or_default()
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

/// Lua errors are expected results here (reported, not printed as panics).
fn quiet_lua_errors() {
    static ONCE: std::sync::Once = std::sync::Once::new();
    if std::env::var_os("INT3_TRACE").is_some() {
        return;
    }
    ONCE.call_once(|| {
        let prev = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |info| {
            if info.payload().downcast_ref::<LuaError>().is_none() {
                prev(info);
            }
        }));
    });
}

/// Run `f`; a Lua error (or any other panic) becomes `Err(message)`.
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

/// Timeless jewel LUTs load from the PoB checkout (as CA / BM-A do).
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

fn custom_corpus() -> bool {
    std::env::var_os("INT3_CORPUS_DIR").is_some_and(|v| !v.is_empty())
}

/// `.parity/builds/*.xml` (or `$INT3_CORPUS_DIR/*.xml`), filtered by `INT3_BUILDS` / `INT3_LIMIT`.
fn corpus(parity: &Path) -> Vec<PathBuf> {
    let dir = if custom_corpus() {
        std::env::var_os("INT3_CORPUS_DIR")
            .map(PathBuf::from)
            .unwrap_or_default()
    } else {
        parity.join("builds")
    };
    let mut v: Vec<PathBuf> = std::fs::read_dir(dir)
        .map(|r| {
            r.filter_map(Result::ok)
                .map(|e| e.path())
                .filter(|p| p.extension().is_some_and(|x| x == "xml"))
                .collect()
        })
        .unwrap_or_default();
    v.sort();
    if let Ok(f) = std::env::var("INT3_BUILDS") {
        let want: Vec<&str> = f.split(',').map(str::trim).collect();
        v.retain(|p| want.iter().any(|w| stem(p) == *w));
    }
    if let Some(n) = std::env::var("INT3_LIMIT")
        .ok()
        .and_then(|s| s.parse().ok())
    {
        v.truncate(n);
    }
    v
}

fn full_corpus(n: usize) -> bool {
    !custom_corpus()
        && std::env::var_os("INT3_BUILDS").is_none()
        && std::env::var_os("INT3_LIMIT").is_none()
        && n == 57
}

/// `INT3_DIFFS=file`: one TSV line per difference.
fn record_diff(cols: &[&str]) {
    let Some(path) = std::env::var_os("INT3_DIFFS") else {
        return;
    };
    let line: Vec<String> = cols.iter().map(|c| c.replace(['\t', '\n'], " ")).collect();
    if let Ok(mut f) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
    {
        let _ = writeln!(f, "{}", line.join("\t"));
    }
}

fn clip(s: &str, n: usize) -> String {
    if s.len() > n {
        let end = s
            .char_indices()
            .take_while(|(i, _)| *i < n)
            .last()
            .map_or(0, |(i, c)| i + c.len_utf8());
        format!("{}…", &s[..end])
    } else {
        s.to_owned()
    }
}

// ---- CalcsTab:BuildOutput as PoB's load runs it (the CA test's scaffold) -----------------------

/// `CalcsTab:BuildOutput()` (CalcsTab.lua 441-472), each calc caught on its own (as `CA.rs`'s
/// scaffold) so that one raising calc still leaves the others to compare.
fn calcsTab_BuildOutput(build: &BuildRef) -> Vec<String> {
    let mut errors = Vec::new();
    build.calcsTab.powerBuildFlag.set(true);
    let spec = build.spec.get();
    for (_, node) in spec.nodes.with(|n| n.pairs()) {
        node.finalModList.set(Some(node.modList.get()));
    }
    match catch(|| buildOutput(build, Mode::MAIN)) {
        Ok(mainEnv) => {
            build
                .calcsTab
                .mainOutput
                .set(Some(mainEnv.player.output.get()));
            build.calcsTab.mainEnv.set(Some(mainEnv));
        }
        Err(e) => errors.push(format!("MAIN: {e}")),
    }
    match catch(|| buildOutput(build, Mode::CALCS)) {
        Ok(calcsEnv) => {
            build
                .calcsTab
                .calcsOutput
                .set(Some(calcsEnv.player.output.get()));
            build.calcsTab.calcsEnv.set(Some(calcsEnv));
        }
        Err(e) => errors.push(format!("CALCS: {e}")),
    }
    match catch(|| getMiscCalculator(build)) {
        Ok((f, _base)) => build.calcsTab.miscCalculator.set(Some(f)),
        Err(e) => errors.push(format!("getMiscCalculator: {e}")),
    }
    errors
}

/// The XML's build model with PoB's load-time calc passes (`Build:Init`'s BuildOutput, then
/// `rounds - 1` `Build:OnFrame` rebuilds; Build.lua 693-697, 1258-1273).
fn rust_load(xml: &str, rounds: usize) -> Result<(BuildRef, Vec<String>), String> {
    let build =
        catch(|| pob_calc::model::build_xml::loadBuildModelFromXML(xml, pob_gamedata::data()))?
            .map_err(|e| format!("load: {e}"))?;
    wipeGlobalCache();
    build.outputRevision.set(1.0);
    let mut errors = calcsTab_BuildOutput(&build);
    for _ in 1..rounds.max(1) {
        wipeGlobalCache();
        build.outputRevision.set(build.outputRevision.get() + 1.0);
        catch(|| build.skillsTab.UpdateSocketGroups(&build))?;
        errors = calcsTab_BuildOutput(&build);
        if let Err(e) = catch(|| getMiscCalculator(&build)) {
            errors.push(format!("configTab GetMiscCalculator: {e}"));
        }
    }
    Ok((build, errors))
}

/// Drop a loaded build. Its envs point back at it (`env.build` is a strong `BuildRef`), so the
/// calcsTab's env and calculator handles are cleared first; otherwise each build stays alive
/// (~20 MB a build).
fn release(build: BuildRef) {
    build.calcsTab.mainEnv.set(None);
    build.calcsTab.calcsEnv.set(None);
    build.calcsTab.mainOutput.set(None);
    build.calcsTab.calcsOutput.set(None);
    build.calcsTab.miscCalculator.set(None);
    drop(build);
}

// ---- PoB's run -----------------------------------------------------------------------------------

// ---- pob-run begin (CK1: this span, from here to its end marker, is part of the cache key)

struct PobRun {
    hits: J,
    int3: J,
    /// `calcsTab.calcsEnv` as a pob-state/1 document.
    calcs: J,
}

/// The source of this file between the pob-run markers (the code that makes a [`PobRun`]).
fn pob_run_code() -> &'static str {
    const SELF: &str = include_str!("INT-3.rs");
    // split literals, so the markers' text does not occur here
    let begin = concat!("// ---- pob-run ", "begin");
    let end = concat!("// ---- pob-run ", "end");
    let a = SELF.find(begin).expect("INT-3.rs: pob-run begin marker");
    let b = SELF[a..].find(end).expect("INT-3.rs: pob-run end marker") + a;
    &SELF[a..b]
}

/// PoB's run of a build (`int3.json`, `calcs.json`, `hits.json`). CK1 (CCR-0146 §4, CCR-0147):
/// an entry of the shared, content-keyed oracle cache (`testkit::oracle_dir`,
/// `.parity/probe-cache/<source key>/<build key>/dir-<key>/`), filled by one PoB run when absent.
/// The key is the patched tree (its sha covers `corpus/CA/stages.toml`, the patcher and the
/// dump/probe Lua; the source key the oracle runtime and the Lua runtime), the build's bytes, its
/// name, `int3.lua`, `runner.rs` (`probe_prelude`) and this span of the test
/// ([`pob_run_code`]), so a harness, dump-format or PoB change runs PoB again. The earlier
/// `target/tmp/int3-probe/<stem>/` was reused whenever its three files were there, whatever made
/// them (and kept a failed run's `error.txt` for good). An `Err` of the run is not cached.
fn pob_run(xml: &Path) -> Result<PobRun, String> {
    use pob_oracle::headless::Engine;
    use pob_oracle::jit::JitPob;
    use pob_oracle::oracle::ORACLE_LUA;
    use pob_oracle::runner::{probe_prelude, OracleLock, ProbeOptions};

    let stem = stem(xml);
    // the CA probes (only their hit counts are read: `ca:399` gives the number of BuildOutputs)
    let stages = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("corpus/CA/stages.toml");
    let patched = pob_oracle::testkit::shared_patch(Some(&stages))?;
    let xml_text = std::fs::read_to_string(xml).map_err(|e| e.to_string())?;
    let entry = pob_oracle::testkit::oracle_dir(
        &patched.src,
        xml_text.as_bytes(),
        &[
            "INT-3 pob_run 1",
            &patched.sha,
            stem,
            INT3_LUA,
            include_str!("../src/runner.rs"),
            pob_run_code(),
        ],
        |dir| {
            std::fs::create_dir_all(dir.join("state")).map_err(|e| e.to_string())?;
            let _lock = OracleLock::acquire().map_err(|e| format!("oracle lock: {e}"))?;
            let out_dir = std::fs::canonicalize(dir).map_err(|e| e.to_string())?;
            let src_root = std::fs::canonicalize(&patched.src).map_err(|e| e.to_string())?;
            let mut root_s = src_root.to_string_lossy().replace('\\', "/");
            if !root_s.ends_with('/') {
                root_s.push('/');
            }
            // no hit is dumped: the calcsEnv document is written by int3.lua after the load
            let opts = ProbeOptions {
                select: vec!["int3:none".to_owned()],
                max_dumps: 0,
                provenance: false,
            };
            let pre = probe_prelude(&root_s, &out_dir, stem, &opts);
            let mut pob = JitPob::boot_oracle(&src_root, Some(&pre))?;
            pob.run(ORACLE_LUA)?;
            pob.load_build_xml(&xml_text, stem)?;
            pob.run("return __probe_finish(__mainObject__.main.modes['BUILD'].calcsTab.mainEnv)")?;
            let r = pob.run(INT3_LUA)?;
            let text = |i: usize| r.get(i).map(|x| x.text().to_owned()).unwrap_or_default();
            pob_oracle::runner::write_atomic(&out_dir.join("calcs.json"), &text(1))
                .map_err(|e| e.to_string())?;
            pob_oracle::runner::write_atomic(&out_dir.join("int3.json"), &text(0))
                .map_err(|e| e.to_string())?;
            Ok(())
        },
    )?;
    let j = |f: &str| -> Result<J, String> {
        let t = std::fs::read_to_string(entry.join(f)).map_err(|e| format!("{f}: {e}"))?;
        serde_json::from_str(&t).map_err(|e| format!("{f}: {e}"))
    };
    Ok(PobRun {
        hits: j("hits.json")?,
        int3: j("int3.json")?,
        calcs: j("calcs.json")?,
    })
}

// ---- pob-run end

/// The Rust model after PoB's number of BuildOutputs (`ca:399` fires twice per BuildOutput).
fn rust_run(xml: &Path, run: &PobRun) -> Result<(BuildRef, Vec<String>), String> {
    let rounds = (run.hits["counts"]["ca:399"].as_u64().unwrap_or(2) / 2) as usize;
    let text = std::fs::read_to_string(xml).unwrap_or_default();
    pob_calc::global_cache::clear_all();
    let r = rust_load(&text, rounds);
    lut::clear_timeless_jewel_luts(None);
    r
}

// ---- breakdown normalisation ----------------------------------------------------------------------

/// `$id` → table of a pob-state/1 tree.
fn id_map(root: &J) -> HashMap<u64, &J> {
    fn collect<'a>(v: &'a J, ids: &mut HashMap<u64, &'a J>) {
        match v {
            J::Object(m) => {
                if let Some(id) = m.get("$id").and_then(J::as_u64) {
                    ids.entry(id).or_insert(v);
                }
                m.values().for_each(|c| collect(c, ids));
            }
            J::Array(a) => a.iter().for_each(|c| collect(c, ids)),
            _ => {}
        }
    }
    let mut ids = HashMap::new();
    collect(root, &mut ids);
    ids
}

const MARKERS: &[&str] = &[
    "$fn",
    "$object",
    "$env",
    "$foreign",
    "$data",
    "$node",
    "$treeNode",
    "$class",
    "$skill",
    "$gem",
    "$gemInstance",
];

/// A breakdown subtree with identities resolved and calc objects reduced to markers.
fn norm(v: &J, ids: &HashMap<u64, &J>, stack: &mut Vec<u64>) -> J {
    match v {
        J::Array(a) => J::Array(a.iter().map(|x| norm(x, ids, stack)).collect()),
        J::Object(m) => {
            if m.len() == 1 {
                if let Some(id) = m.get("$ref").and_then(J::as_u64) {
                    if stack.contains(&id) || stack.len() > 40 {
                        return serde_json::json!({ "$cycle": true });
                    }
                    let Some(t) = ids.get(&id) else {
                        return v.clone();
                    };
                    stack.push(id);
                    let r = norm(t, ids, stack);
                    stack.pop();
                    return r;
                }
            }
            if let Some(name) = m.get("$item") {
                return serde_json::json!({ "$item": name });
            }
            if let Some(cls) = m.get("$store") {
                return serde_json::json!({ "$store": cls });
            }
            if MARKERS.iter().any(|k| m.contains_key(*k)) {
                let mut o = m.clone();
                o.remove("$id");
                return J::Object(o);
            }
            // an actor or an active skill reached from a breakdown row
            if m.contains_key("modDB") && m.contains_key("output") {
                return serde_json::json!({ "$actor": true });
            }
            if m.contains_key("activeEffect") && m.contains_key("skillFlags") {
                return serde_json::json!({ "$activeSkill": true });
            }
            let pushed = m.get("$id").and_then(J::as_u64);
            if let Some(id) = pushed {
                stack.push(id);
            }
            let mut o: Map<String, J> = m
                .iter()
                .filter(|(k, _)| k.as_str() != "$id")
                .map(|(k, x)| (k.clone(), norm(x, ids, stack)))
                .collect();
            if pushed.is_some() {
                stack.pop();
            }
            // A mod without tags: both encoders write a mod reached as a calc table with
            // `"tags": []`, but one reached inside a data value (a LIST mod's `value.mod`, e.g.
            // the `value` of `MinionModifier` rows of `SkillBuffs`, Calcs.lua:765-770) without the
            // member (state_dump.lua `encodeMod` vs its plain values; `pob_calc::probe::
            // plain_mod_json`). An absent `tags` is the empty tag list either way (H3; the
            // helper is shared with CA's L5 compare, FUP4).
            pob_oracle::mod_json::default_empty_tags(&mut o);
            J::Object(o)
        }
        other => other.clone(),
    }
}

/// The array part of an encoded table: a JSON array, or an object's `$seq`.
fn seq_mut(v: &mut J) -> Option<&mut Vec<J>> {
    match v {
        J::Array(a) => Some(a),
        J::Object(m) => m.get_mut("$seq").and_then(J::as_array_mut),
        _ => None,
    }
}

/// A JSON value with every number as an f64 and object keys sorted, as a string (sort key).
fn canon(j: &J) -> String {
    fn n(j: &J) -> J {
        match j {
            J::Number(x) => J::String(format!("#{:?}", x.as_f64().unwrap_or(f64::NAN))),
            J::Array(a) => J::Array(a.iter().map(n).collect()),
            J::Object(m) => {
                let s: BTreeMap<String, J> = m.iter().map(|(k, v)| (k.clone(), n(v))).collect();
                J::Object(s.into_iter().collect())
            }
            o => o.clone(),
        }
    }
    n(j).to_string()
}

/// The lists the breakdown control sorts before showing them, sorted by their canonical text
/// (CalcBreakdownControl.lua 181-190 `rowList` of an entry with a `source`, 263-265 `slots`,
/// 337-345 `modList`).
///
/// Also the lists whose order is PoB's per-process `pairs` order (LUA-ORDER, calc doc §3.4):
/// `<type>MaximumHitTaken.rowList` (`pairs(takenDamages)`, string keys, CalcDefence.lua:3325), and
/// `TotalBuildDegen.rowList` (`pairs(actor.damageOverTimeShiftTable[damageType])`, string keys,
/// CalcDefence.lua:3389: e.g. allflame_exp_002's Physical / Fire rows swap between PoB processes).
fn ui_sorted(key: &str, entry: &J) -> J {
    let mut e = entry.clone();
    let has_source = e.get("source").is_some_and(|s| !s.is_null())
        || key.ends_with("MaximumHitTaken")
        || key == "TotalBuildDegen";
    if let J::Object(m) = &mut e {
        for (k, v) in m.iter_mut() {
            let sort = k == "slots" || k == "modList" || (k == "rowList" && has_source);
            if sort {
                if let Some(list) = seq_mut(v) {
                    list.sort_by_cached_key(canon);
                }
            }
        }
    }
    e
}

/// Accepted differences, counted ([`reconcile`]).
#[derive(Default)]
struct Accepted {
    /// `string.format("%d", x)` of a NaN / ±inf `x` (host artefact, CCR-0001 / AMENDMENTS A6;
    /// known_diffs.toml `DF-E-1`): `-9223372036854775808` on x64 PoB (`cvttsd2si`, the profile
    /// the port follows), saturated on the ARM64 dual-number oracle (`0` for NaN,
    /// `±9223372036854775807` for ±inf). E.g. `FrostShieldLife[5]`, CalcDefence.lua:2498.
    /// Only accepted against an ARM64 oracle (this host).
    int_cast: usize,
    /// A Lua closure stored in a breakdown row (`sourceNameTooltip`, CalcPerform.lua:2139-2141):
    /// a calc object (`Val::Obj`) in Rust, which the state export writes as `{"$userdata": true}`
    /// while PoB's dump writes `{"$fn": "<file>:<lines>"}`. Its presence is compared, not its source.
    closure: usize,
    /// PoB's own nondeterminism ([`POB_NONDET_NAMES`]): a string that differs only by a name PoB
    /// picks in per-process `pairs` order.
    pob_nondet: usize,
}

/// `INT3_STRICT=1`: accept none of PoB's nondeterministic choices.
fn strict() -> bool {
    std::env::var_os("INT3_STRICT").is_some_and(|v| v == "1")
}

/// Names PoB picks in per-process `pairs` order, as (PoB's alternative, the one Rust picks):
/// * an old-format gem (`skillId`, no `variantId`) takes the first variant of
///   `pairs(possibleVariants)` whose `grantedEffectId` matches (SkillsTab.lua:472-478, string
///   keys), and Data.lua:1127-1136 gives the synthesised "Vaal Righteous Fire of Arcane Devotion"
///   the base gem's `gameId` and `grantedEffectId` (CCR-0104 "PoB nondeterminism").
const POB_NONDET_NAMES: &[(&str, &str)] = &[(
    "Vaal Righteous Fire of Arcane Devotion",
    "Vaal Righteous Fire",
)];

fn pob_nondet_eq(p: &str, o: &str) -> bool {
    !strict()
        && POB_NONDET_NAMES
            .iter()
            .any(|(alt, ours)| p.contains(alt) && p.replace(alt, ours) == o)
}

/// Make `ours` equal `pob` at the places where they differ only by an [`Accepted`] class.
fn reconcile(pob: &J, ours: &mut J, acc: &mut Accepted) {
    const X64_MIN: &str = "-9223372036854775808";
    match (pob, &mut *ours) {
        (J::String(p), J::String(o)) => {
            if cfg!(target_arch = "aarch64")
                && o.contains(X64_MIN)
                && ["0", "9223372036854775807", "-9223372036854775807"]
                    .iter()
                    .any(|alt| o.replace(X64_MIN, alt) == *p)
            {
                *o = p.clone();
                acc.int_cast += 1;
            } else if p != o && pob_nondet_eq(p, o) {
                *o = p.clone();
                acc.pob_nondet += 1;
            }
        }
        (J::Array(p), J::Array(o)) => {
            for (a, b) in p.iter().zip(o.iter_mut()) {
                reconcile(a, b, acc);
            }
        }
        (J::Object(p), J::Object(o)) => {
            if p.contains_key("$fn") && o.len() == 1 && o.get("$userdata") == Some(&J::Bool(true)) {
                *ours = pob.clone();
                acc.closure += 1;
                return;
            }
            for (k, a) in p {
                if let Some(b) = o.get_mut(k) {
                    reconcile(a, b, acc);
                }
            }
        }
        _ => {}
    }
}

/// The breakdown entries of an actor: top-level keys, and the keys of the `MainHand` / `OffHand`
/// namespaces (`ns.name`).
fn entries(bd: &J) -> BTreeMap<String, J> {
    let mut out = BTreeMap::new();
    let Some(m) = bd.as_object() else { return out };
    for (k, v) in m {
        if k.starts_with('$') {
            continue;
        }
        if (k == "MainHand" || k == "OffHand") && v.is_object() {
            for (k2, v2) in v.as_object().into_iter().flatten() {
                if !k2.starts_with('$') {
                    out.insert(format!("{k}.{k2}"), v2.clone());
                }
            }
        } else {
            out.insert(k.clone(), v.clone());
        }
    }
    out
}

/// An actor's normalised breakdown entries in a pob-state/1 root (`env.<actor>.breakdown`).
fn actor_entries(root: &J, actor: &str) -> Option<BTreeMap<String, J>> {
    let ids = id_map(root);
    let mut stack = Vec::new();
    let env = norm_shallow(&root["env"], &ids);
    let a = norm_shallow(env.get(actor)?, &ids);
    let bd = a.get("breakdown")?;
    if bd.is_null() {
        return None;
    }
    let bd = norm(bd, &ids, &mut stack);
    Some(entries(&bd))
}

/// The normalised breakdown entries of a root exported with the breakdown as local `name`.
fn local_entries(root: &J, name: &str) -> Option<BTreeMap<String, J>> {
    let ids = id_map(root);
    let bd = norm_shallow(root.get("locals")?.get(name)?, &ids);
    if bd.is_null() {
        return None;
    }
    let bd = norm(bd, &ids, &mut Vec::new());
    Some(entries(&bd))
}

/// A `$ref` resolved one level.
fn norm_shallow<'a>(v: &'a J, ids: &HashMap<u64, &'a J>) -> &'a J {
    let mut v = v;
    for _ in 0..8 {
        match v
            .as_object()
            .filter(|m| m.len() == 1)
            .and_then(|m| m.get("$ref"))
            .and_then(J::as_u64)
        {
            Some(id) => match ids.get(&id) {
                Some(t) => v = t,
                None => break,
            },
            None => break,
        }
    }
    v
}

#[derive(Default)]
struct BdTotals {
    builds: usize,
    entries: usize,
    equal: usize,
    exact: usize,
    pob_only: usize,
    rust_only: usize,
    accepted: Accepted,
    rust_errors: Vec<String>,
    failed_builds: Vec<String>,
    /// mismatching entries by entry key (across builds).
    by_key: BTreeMap<String, usize>,
}

#[test]
fn int3_breakdown_corpus() {
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
    let builds = corpus(&parity);
    if builds.is_empty() {
        return skip(".parity/builds/*.xml not found");
    }
    let mut t = BdTotals::default();
    let mut pob_failed = Vec::new();
    for xml in &builds {
        let stem = stem(xml);
        let run = match pob_run(xml) {
            Ok(r) => r,
            Err(e) => {
                pob_failed.push(format!("{stem}: {e}"));
                continue;
            }
        };
        t.builds += 1;
        let (build, errors) = match rust_run(xml, &run) {
            Ok(r) => r,
            Err(e) => {
                eprintln!("INT3 {stem}: Rust {e}");
                t.rust_errors.push(format!("{stem}: {e}"));
                continue;
            }
        };
        for e in errors {
            eprintln!("INT3 {stem}: Rust {e}");
            t.rust_errors.push(format!("{stem}: {e}"));
        }
        let Some(calcsEnv) = build.calcsTab.calcsEnv.get() else {
            t.failed_builds.push(format!("{stem}: no Rust calcsEnv"));
            continue;
        };
        // only the two breakdowns are exported (as locals), not the whole env
        let locals: Vec<(String, Val)> = [
            ("player", Some(calcsEnv.player.clone())),
            ("minion", calcsEnv.minion.get()),
        ]
        .into_iter()
        .filter_map(|(n, a)| {
            a.and_then(|a| a.breakdown.get())
                .map(|b| (n.to_owned(), Val::Obj(Rc::new(b))))
        })
        .collect();
        let ours_root = match catch(|| pob_calc::probe::export_root(None, &locals)) {
            Ok(r) => r,
            Err(e) => {
                t.rust_errors.push(format!("{stem}: export: {e}"));
                continue;
            }
        };
        let pob_root = &run.calcs["root"];
        let mut build_bad = 0;
        let mut build_n = 0;
        for actor in ["player", "minion"] {
            let p = actor_entries(pob_root, actor);
            let o = local_entries(&ours_root, actor);
            let (p, o) = match (p, o) {
                (None, None) => continue,
                (p, o) => (p.unwrap_or_default(), o.unwrap_or_default()),
            };
            let keys: BTreeSet<&String> = p.keys().chain(o.keys()).collect();
            for k in keys {
                t.entries += 1;
                build_n += 1;
                let pe = p.get(k);
                let oe = o.get(k).map(|b| {
                    let mut b = b.clone();
                    if let Some(a) = pe {
                        let before = (t.accepted.int_cast, t.accepted.pob_nondet);
                        reconcile(a, &mut b, &mut t.accepted);
                        if t.accepted.int_cast > before.0 {
                            record_diff(&[stem, "accepted-int-cast", actor, k, "", "", ""]);
                        }
                        if t.accepted.pob_nondet > before.1 {
                            record_diff(&[stem, "accepted-pob-nondet", actor, k, "", "", ""]);
                        }
                    }
                    b
                });
                let ok = match (pe, oe.as_ref()) {
                    (Some(a), Some(b)) => {
                        if tree_eq(a, b, REL_TOL) {
                            t.exact += 1;
                            t.equal += 1;
                            true
                        } else if tree_eq(&ui_sorted(k, a), &ui_sorted(k, b), REL_TOL) {
                            t.equal += 1;
                            let lists: Vec<String> = diff_trees(a, b, &DiffOptions::strict())
                                .iter()
                                .map(|m| m.path.split('[').next().unwrap_or_default().to_owned())
                                .collect::<BTreeSet<_>>()
                                .into_iter()
                                .collect();
                            record_diff(&[
                                stem,
                                "breakdown-order",
                                actor,
                                k,
                                &lists.join(","),
                                "",
                                "",
                            ]);
                            true
                        } else {
                            let mm = diff_trees(
                                &ui_sorted(k, a),
                                &ui_sorted(k, b),
                                &DiffOptions::strict(),
                            );
                            for m in mm.iter().take(if verbose() { 40 } else { 3 }) {
                                eprintln!(
                                    "    {stem} {actor}.{k}{} {:?}: pob={} ours={}",
                                    m.path,
                                    m.kind,
                                    clip(
                                        &m.pob.as_ref().map(J::to_string).unwrap_or_default(),
                                        160
                                    ),
                                    clip(
                                        &m.ours.as_ref().map(J::to_string).unwrap_or_default(),
                                        160
                                    )
                                );
                            }
                            for m in &mm {
                                record_diff(&[
                                    stem,
                                    "breakdown",
                                    actor,
                                    k,
                                    &m.path,
                                    &clip(
                                        &m.pob.as_ref().map(J::to_string).unwrap_or_default(),
                                        400,
                                    ),
                                    &clip(
                                        &m.ours.as_ref().map(J::to_string).unwrap_or_default(),
                                        400,
                                    ),
                                ]);
                            }
                            false
                        }
                    }
                    (Some(a), None) => {
                        t.pob_only += 1;
                        eprintln!(
                            "    {stem} {actor}.{k}: missing in Rust (PoB {})",
                            clip(&a.to_string(), 160)
                        );
                        record_diff(&[
                            stem,
                            "breakdown",
                            actor,
                            k,
                            "",
                            &clip(&a.to_string(), 400),
                            "<missing>",
                        ]);
                        false
                    }
                    (None, Some(b)) => {
                        t.rust_only += 1;
                        eprintln!(
                            "    {stem} {actor}.{k}: only in Rust ({})",
                            clip(&b.to_string(), 160)
                        );
                        record_diff(&[
                            stem,
                            "breakdown",
                            actor,
                            k,
                            "",
                            "<missing>",
                            &clip(&b.to_string(), 400),
                        ]);
                        false
                    }
                    (None, None) => true,
                };
                if !ok {
                    build_bad += 1;
                    *t.by_key.entry(format!("{actor}.{k}")).or_default() += 1;
                }
            }
        }
        eprintln!(
            "INT3 breakdown {stem}: {}/{build_n} entries equal",
            build_n - build_bad
        );
        if build_bad > 0 {
            t.failed_builds
                .push(format!("{stem}: {build_bad} entries differ"));
        }
        drop(ours_root);
        drop(calcsEnv);
        release(build);
        pob_calc::global_cache::clear_all();
    }
    eprintln!(
        "INT3 breakdown: {} builds ({} PoB runs failed), {} Rust errors; entries {}/{} equal ({} exact in order), \
         {} missing in Rust, {} only in Rust; {} builds with a difference; accepted: {} %d host casts, {} closures, {} PoB-nondeterministic names",
        t.builds,
        pob_failed.len(),
        t.rust_errors.len(),
        t.equal,
        t.entries,
        t.exact,
        t.pob_only,
        t.rust_only,
        t.failed_builds.len(),
        t.accepted.int_cast,
        t.accepted.closure,
        t.accepted.pob_nondet
    );
    let mut worst: Vec<(&String, &usize)> = t.by_key.iter().collect();
    worst.sort_by(|a, b| b.1.cmp(a.1).then(a.0.cmp(b.0)));
    for (k, n) in worst.iter().take(40) {
        eprintln!("    differing entry {k}: {n} builds");
    }
    for f in &pob_failed {
        eprintln!("    PoB failed: {f}");
    }
    for e in &t.rust_errors {
        eprintln!("    Rust error: {e}");
    }
    assert!(pob_failed.is_empty(), "PoB runs failed: {pob_failed:?}");
    assert!(t.rust_errors.is_empty(), "Rust errors: {:?}", t.rust_errors);
    if full_corpus(builds.len()) {
        assert!(
            t.entries >= BD_FULL_ENTRIES,
            "breakdown entries compared {} < pinned {BD_FULL_ENTRIES}",
            t.entries
        );
    }
    assert!(
        t.equal == t.entries,
        "breakdown entries differ: {:?}",
        t.failed_builds
    );
}

/// Breakdown entries compared on the full corpus (docs/port/parity/INT-3-triage.md).
const BD_FULL_ENTRIES: usize = 10_606;

// ---- Tabulate ---------------------------------------------------------------------------------------

fn val_json(v: &Val) -> J {
    match v {
        Val::Nil => J::Null,
        Val::Bool(b) => J::Bool(*b),
        Val::Num(x) => {
            serde_json::Number::from_f64(*x).map_or_else(|| J::String(format!("{x}")), J::Number)
        }
        Val::Str(s) => J::String(s.to_string()),
        Val::Data(d) => pob_calc::probe::value_json(d),
        Val::Mod(m) => pob_calc::probe::mod_json(m, None),
        Val::Table(t) => {
            let n = t.len();
            let pairs: Vec<(Key, Val)> = t.pairs().collect();
            if n > 0 && pairs.len() == n {
                J::Array((1..=n).map(|i| val_json(&t.get(i as f64))).collect())
            } else {
                let mut m = Map::new();
                for (k, x) in pairs {
                    let ks = match k {
                        Key::S(s) => s.to_string(),
                        Key::I(i) => i.to_string(),
                        Key::B(b) => b.to_string(),
                        Key::F(f) => tostring_num(f.get()),
                    };
                    m.insert(ks, val_json(&x));
                }
                J::Object(m)
            }
        }
        other => J::String(format!("<{}>", other.lua_type())),
    }
}

/// An oracle-JSON list (`__oracle_json` writes an empty Lua table as `{}`).
fn json_list(j: &J) -> Vec<J> {
    j.as_array().cloned().unwrap_or_default()
}

/// An oracle-JSON value made comparable with ours: numbers as f64 text, keys sorted.
fn tab_canon(j: &J) -> String {
    canon(j)
}

/// `actor.mainSkill[name]` for a cfg name (the typed cfg fields, then the computed ones in
/// `extra`, e.g. `"dot" .. damageType .. "Cfg"`, CalcOffence.lua:5812), as U40's
/// `tabs::calcs::breakdown::main_skill_cfg`.
fn main_skill_cfg(skill: &ActiveSkill, name: &str) -> Option<Cfg> {
    match name {
        "skillCfg" => Some(skill.skillCfg.get()),
        "weapon1Cfg" | "weapon2Cfg" | "bleedCfg" | "poisonCfg" | "igniteCfg" | "OHbleedCfg"
        | "OHpoisonCfg" | "OHigniteCfg" | "dotCfg" | "decayCfg" => {
            skill.cfg_field(Name::from(name)).get()
        }
        _ => skill.extra.get(Name::from(name)).obj::<ModCfg>(),
    }
}

struct Query {
    sig: String,
    names: Vec<Name>,
    modType: Option<String>,
    cfg: Option<String>,
    enemy: bool,
    actor: Option<String>,
    modSource: Option<String>,
}

fn queries(int3: &J) -> Vec<Query> {
    let s = |v: &J| v.as_str().map(str::to_owned);
    int3["queries"]
        .as_array()
        .map(|a| {
            a.iter()
                .map(|q| Query {
                    sig: s(&q["sig"]).unwrap_or_default(),
                    names: q["modName"]
                        .as_array()
                        .map(|l| l.iter().filter_map(J::as_str).map(Name::from).collect())
                        .unwrap_or_default(),
                    modType: s(&q["modType"]),
                    cfg: s(&q["cfg"]),
                    enemy: q["enemy"].as_bool().unwrap_or(false),
                    actor: s(&q["actor"]),
                    modSource: s(&q["modSource"]),
                })
                .collect()
        })
        .unwrap_or_default()
}

/// CalcBreakdownControl.lua 303-318 and 348-381 on a Rust actor: (rows, totals) as oracle JSON.
fn run_query(actor: &ActorRef, q: &Query) -> (Vec<J>, Vec<J>) {
    // L305
    let cfg: Cfg = match &q.cfg {
        Some(c) => {
            let skill = actor
                .mainSkill
                .get()
                .unwrap_or_else(|| lua_error!("attempt to index field 'mainSkill' (a nil value)"));
            match main_skill_cfg(&skill, &format!("{c}Cfg")) {
                Some(c) => c.copyTable(true),
                None => ModCfg::new(),
            }
        }
        None => ModCfg::new(),
    };
    // L306-307
    cfg.source.set(q.modSource.as_deref().map(Name::from));
    cfg.actor.set(q.actor.as_deref().map(Name::from));
    // L309
    let modStore: ModStoreRef = if q.enemy {
        actor
            .enemy()
            .unwrap_or_else(|| lua_error!("attempt to index field 'enemy' (a nil value)"))
            .modDB
            .get()
    } else if q.cfg.is_some() {
        actor
            .mainSkill
            .get()
            .unwrap_or_else(|| lua_error!("attempt to index field 'mainSkill' (a nil value)"))
            .skillModList
            .get()
    } else {
        actor.modDB.get()
    };
    let modType = q
        .modType
        .as_deref()
        .map(|s| ModType::parse(s).unwrap_or(ModType::Nil));
    // L313-317
    let rows = modStore.Tabulate(modType, Some(&*cfg), &q.names);
    let rows_json: Vec<J> = rows
        .iter()
        .map(|r| {
            serde_json::json!({
                "mod": pob_calc::probe::mod_json(&r.r#mod, None),
                "value": pob_calc::probe::value_json(&r.value),
            })
        })
        .collect();
    // L348-381
    let mut totals = Vec::new();
    if q.modSource.is_none() {
        let mut typeList: Vec<ModType> = Vec::new();
        let mut sources: BTreeSet<String> = BTreeSet::new();
        for r in &rows {
            if !typeList.contains(&r.r#mod.r#type) {
                typeList.push(r.r#mod.r#type);
            }
            let src = r
                .r#mod
                .source
                .as_ref()
                .map(|s| s.to_string())
                .unwrap_or_else(|| lua_error!("attempt to index field 'source' (a nil value)"));
            let st = src.split(':').next().unwrap_or_default().to_owned();
            sources.insert(st);
        }
        for st in &sources {
            cfg.source.set(Some(Name::from(st.as_str())));
            for mt in &typeList {
                for name in &q.names {
                    let total = modStore.Combine(*mt, Some(&*cfg), std::slice::from_ref(name));
                    // (a nil total is an absent key in PoB's JSON)
                    let mut o = serde_json::json!({
                        "source": st,
                        "modType": mt.as_str(),
                        "modName": name.as_str(),
                    });
                    if !total.is_nil() {
                        o["total"] = val_json(&total);
                    }
                    totals.push(o);
                }
            }
        }
    }
    (rows_json, totals)
}

#[derive(Default)]
struct TabTotals {
    builds: usize,
    queries: usize,
    equal: usize,
    order_only: usize,
    errors_both: usize,
    rows: usize,
    /// queries equal only up to PoB's nondeterministic mod sources ([`nondet_row`]).
    pob_nondet: usize,
    rust_errors: Vec<String>,
    failed_builds: Vec<String>,
    by_query: BTreeMap<String, usize>,
}

#[test]
fn int3_tabulate_corpus() {
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
    let builds = corpus(&parity);
    if builds.is_empty() {
        return skip(".parity/builds/*.xml not found");
    }
    let mut t = TabTotals::default();
    let mut pob_failed = Vec::new();
    for xml in &builds {
        let stem = stem(xml);
        let run = match pob_run(xml) {
            Ok(r) => r,
            Err(e) => {
                pob_failed.push(format!("{stem}: {e}"));
                continue;
            }
        };
        t.builds += 1;
        let (build, errors) = match rust_run(xml, &run) {
            Ok(r) => r,
            Err(e) => {
                t.rust_errors.push(format!("{stem}: {e}"));
                continue;
            }
        };
        for e in errors {
            t.rust_errors.push(format!("{stem}: {e}"));
        }
        let Some(calcsEnv) = build.calcsTab.calcsEnv.get() else {
            t.failed_builds.push(format!("{stem}: no Rust calcsEnv"));
            continue;
        };
        let qs = queries(&run.int3);
        let mut bad = 0;
        let mut n = 0;
        for (actor_name, actor) in [
            ("player", Some(calcsEnv.player.clone())),
            ("minion", calcsEnv.minion.get()),
        ] {
            let pob = &run.int3[actor_name];
            if pob.is_null() {
                if actor.is_some() && actor_name == "minion" {
                    bad += 1;
                    t.failed_builds
                        .push(format!("{stem}: a Rust minion, none in PoB"));
                }
                continue;
            }
            let Some(actor) = actor else {
                bad += 1;
                t.failed_builds
                    .push(format!("{stem}: no Rust {actor_name}"));
                continue;
            };
            for q in &qs {
                t.queries += 1;
                n += 1;
                let p = &pob[q.sig.as_str()];
                let ours = catch(|| run_query(&actor, q));
                lut::clear_timeless_jewel_luts(None);
                let diff: Option<String> = match (p.get("error"), ours) {
                    (Some(_), Err(_)) => {
                        t.errors_both += 1;
                        None
                    }
                    (Some(e), Ok(_)) => Some(format!(
                        "PoB raised ({}), Rust did not",
                        clip(&e.to_string(), 120)
                    )),
                    (None, Err(e)) => Some(format!("Rust raised: {e}")),
                    (None, Ok((rows, totals))) => {
                        let mut prows: Vec<String> =
                            json_list(&p["rows"]).iter().map(tab_canon).collect();
                        let mut orows: Vec<String> = rows.iter().map(tab_canon).collect();
                        if rows_equal(&prows, &orows) == RowsEq::Differ && !strict() {
                            let pn: Vec<String> = json_list(&p["rows"])
                                .iter()
                                .map(|r| tab_canon(&nondet_row(r, &build)))
                                .collect();
                            let on: Vec<String> = rows
                                .iter()
                                .map(|r| tab_canon(&nondet_row(r, &build)))
                                .collect();
                            if rows_equal(&pn, &on) != RowsEq::Differ {
                                t.pob_nondet += 1;
                                record_diff(&[
                                    stem,
                                    "accepted-pob-nondet",
                                    actor_name,
                                    &q.sig,
                                    "",
                                    "",
                                    "",
                                ]);
                                prows = pn;
                                orows = on;
                            }
                        }
                        t.rows += prows.len();
                        let ptot_j = json_list(&p["totals"]);
                        let mut ptot: Vec<String> = ptot_j.iter().map(tab_canon).collect();
                        let mut otot: Vec<String> = totals.iter().map(tab_canon).collect();
                        ptot.sort();
                        otot.sort();
                        let rows_eq = rows_equal(&prows, &orows);
                        let tot_eq = ptot.len() == otot.len() && {
                            let mut pa: Vec<&J> = ptot_j.iter().collect();
                            let mut oa: Vec<&J> = totals.iter().collect();
                            pa.sort_by_cached_key(|x| tab_canon(x));
                            oa.sort_by_cached_key(|x| tab_canon(x));
                            pa.iter().zip(&oa).all(|(a, b)| tree_eq(a, b, REL_TOL))
                        };
                        match (rows_eq, tot_eq) {
                            (RowsEq::Exact, true) => None,
                            (RowsEq::OrderOnly, true) => {
                                t.order_only += 1;
                                record_diff(&[
                                    stem,
                                    "tabulate-order",
                                    actor_name,
                                    &q.sig,
                                    "",
                                    &prows.join(" ;; "),
                                    &orows.join(" ;; "),
                                ]);
                                None
                            }
                            (r, tot) => {
                                let mut s = String::new();
                                if r == RowsEq::Differ {
                                    let po: BTreeSet<&String> = prows.iter().collect();
                                    let oo: BTreeSet<&String> = orows.iter().collect();
                                    let miss: Vec<&&String> = po.difference(&oo).take(2).collect();
                                    let extra: Vec<&&String> = oo.difference(&po).take(2).collect();
                                    s.push_str(&format!(
                                        "rows {} vs PoB {}; PoB-only {:?}; Rust-only {:?}",
                                        orows.len(),
                                        prows.len(),
                                        miss.iter().map(|x| clip(x, 300)).collect::<Vec<_>>(),
                                        extra.iter().map(|x| clip(x, 300)).collect::<Vec<_>>()
                                    ));
                                }
                                if !tot {
                                    let miss: Vec<&String> =
                                        ptot.iter().filter(|x| !otot.contains(x)).take(2).collect();
                                    let extra: Vec<&String> =
                                        otot.iter().filter(|x| !ptot.contains(x)).take(2).collect();
                                    s.push_str(&format!(
                                        " totals: PoB-only {miss:?}; Rust-only {extra:?}"
                                    ));
                                }
                                Some(s)
                            }
                        }
                    }
                };
                if let Some(d) = diff {
                    bad += 1;
                    *t.by_query
                        .entry(format!("{actor_name} {}", q.sig))
                        .or_default() += 1;
                    eprintln!("    {stem} {actor_name} [{}]: {d}", q.sig);
                    record_diff(&[stem, "tabulate", actor_name, &q.sig, "", &d, ""]);
                } else {
                    t.equal += 1;
                }
            }
        }
        eprintln!("INT3 tabulate {stem}: {}/{n} queries equal", n - bad);
        if bad > 0 {
            t.failed_builds
                .push(format!("{stem}: {bad} queries differ"));
        }
        drop(calcsEnv);
        release(build);
        pob_calc::global_cache::clear_all();
    }
    eprintln!(
        "INT3 tabulate: {} builds ({} PoB runs failed), {} Rust errors; queries {}/{} equal ({} equal up to row order, \
         {} raise on both sides, {} equal up to PoB's nondeterministic sources), {} PoB rows; {} builds with a difference",
        t.builds,
        pob_failed.len(),
        t.rust_errors.len(),
        t.equal,
        t.queries,
        t.order_only,
        t.errors_both,
        t.pob_nondet,
        t.rows,
        t.failed_builds.len()
    );
    let mut worst: Vec<(&String, &usize)> = t.by_query.iter().collect();
    worst.sort_by(|a, b| b.1.cmp(a.1).then(a.0.cmp(b.0)));
    for (k, n) in worst.iter().take(30) {
        eprintln!("    differing query {k}: {n} builds");
    }
    for f in &pob_failed {
        eprintln!("    PoB failed: {f}");
    }
    for e in &t.rust_errors {
        eprintln!("    Rust error: {e}");
    }
    assert!(pob_failed.is_empty(), "PoB runs failed: {pob_failed:?}");
    assert!(t.rust_errors.is_empty(), "Rust errors: {:?}", t.rust_errors);
    if full_corpus(builds.len()) {
        assert!(
            t.queries >= TAB_FULL_QUERIES,
            "Tabulate queries compared {} < pinned {TAB_FULL_QUERIES}",
            t.queries
        );
        assert!(
            t.rows >= TAB_FULL_ROWS,
            "Tabulate rows compared {} < pinned {TAB_FULL_ROWS}",
            t.rows
        );
    }
    assert!(
        t.equal == t.queries,
        "Tabulate queries differ: {:?}",
        t.failed_builds
    );
}

/// A Tabulate row with the mod sources PoB picks in per-process order replaced by a class name:
/// * `Item:<id>:<name>` of a flask → `Item:<flask>`: `mergeFlasks` walks `pairs(flasks)`
///   (item-keyed, address order in PoB; CalcPerform.lua:1773) and `mergeBuff` keeps the first of
///   two flasks with the same buff key (CalcPerform.lua 1731/1758, key = base name or the mods'
///   `formatModParams`), so which flask's mod reaches the modDB is PoB's per-process choice;
/// * `Skill:ExplosiveTrapAltX` → `Skill:ExplosiveTrap`: the two share one `statMap` table
///   (act_dex.lua:7326), and Data.lua:1049 and 1065-1079 stamps its mods' `source` from whichever of them
///   `pairs(data.skills)` (string keys) reaches last.
fn nondet_row(row: &J, build: &BuildRef) -> J {
    let mut r = row.clone();
    let Some(src) = r["mod"]["source"].as_str().map(str::to_owned) else {
        return r;
    };
    let new = if let Some(rest) = src.strip_prefix("Item:") {
        let id = rest.split(':').next().and_then(|x| x.parse::<u32>().ok());
        let is_flask = id
            .and_then(|id| build.itemsTab.items.with(|m| m.get(&id).cloned()))
            .is_some_and(|it| it.r#type.get().as_str() == "Flask");
        is_flask.then(|| "Item:<flask>".to_owned())
    } else if src == "Skill:ExplosiveTrapAltX" {
        Some("Skill:ExplosiveTrap".to_owned())
    } else {
        None
    };
    if let Some(n) = new {
        r["mod"]["source"] = J::String(n);
    }
    r
}

/// Tabulate queries and PoB rows compared on the full corpus (docs/port/parity/INT-3-triage.md).
const TAB_FULL_QUERIES: usize = 38_940;
const TAB_FULL_ROWS: usize = 42_588;

#[derive(PartialEq, Eq, Debug, Clone, Copy)]
enum RowsEq {
    Exact,
    OrderOnly,
    Differ,
}

/// Tabulate rows (canonical texts) in order, else as multisets. The texts carry numbers as
/// their f64 debug text, so equality here is exact; the §9.2 tolerance is applied by
/// [`rows_close`] when the texts differ.
fn rows_equal(p: &[String], o: &[String]) -> RowsEq {
    if p == o {
        return RowsEq::Exact;
    }
    let (mut ps, mut os) = (p.to_vec(), o.to_vec());
    ps.sort();
    os.sort();
    if ps == os {
        return RowsEq::OrderOnly;
    }
    if rows_close(p, o) {
        return RowsEq::Exact;
    }
    RowsEq::Differ
}

/// Rows equal within the §9.2 numeric tolerance, in order.
fn rows_close(p: &[String], o: &[String]) -> bool {
    p.len() == o.len()
        && p.iter().zip(o).all(|(a, b)| {
            let (Ok(a), Ok(b)) = (serde_json::from_str::<J>(a), serde_json::from_str::<J>(b))
            else {
                return false;
            };
            tree_eq(&un_canon(&a), &un_canon(&b), REL_TOL)
        })
}

/// [`canon`]'s `"#<f64>"` strings back to numbers.
fn un_canon(j: &J) -> J {
    match j {
        J::String(s) if s.starts_with('#') => s[1..]
            .parse::<f64>()
            .ok()
            .and_then(serde_json::Number::from_f64)
            .map_or_else(|| j.clone(), J::Number),
        J::Array(a) => J::Array(a.iter().map(un_canon).collect()),
        J::Object(m) => J::Object(m.iter().map(|(k, v)| (k.clone(), un_canon(v))).collect()),
        o => o.clone(),
    }
}
