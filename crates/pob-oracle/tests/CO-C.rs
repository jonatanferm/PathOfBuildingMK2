//! Unit CO-C acceptance (calc doc §11.1, row CO-C): `ConfigTab:BuildModList`
//! (Classes/ConfigTab.lua 1130-1203, `pob_calc::model::config_build_mod_list`) and the
//! Data/ModMap.lua map mod applies (`pob_calc::modules::mod_map`) against PoB's own under
//! LuaJIT (the oracle, `jit.off()`), driven by `corpus/CO-C/bml_probe.lua`.
//!
//! * `corpus_build_mod_list_matches_oracle` — every `.parity/builds/*.xml` build is loaded into
//!   PoB (one build per oracle boot, so only one PoB lives at a time); the probe records the
//!   state BuildModList reads and the applies write (the active config set's `input` /
//!   `placeholder` / `customModsList`, every `varControls` control field, `enemyLevel`,
//!   `characterLevel`, `buildFlag`), runs `configTab:BuildModList()` under `pcall` and records
//!   `configTab.modList` / `enemyModList` and the post-state. Rust restores the pre-state into a
//!   model `Build`, runs `ConfigTab::BuildModList` and must reproduce the error status, both
//!   lists (mods in order: name, type, value, flags, keywordFlags, tags, source) and the
//!   post-state. The one `pairs` loop BuildModList reaches (`presetBossSkills` adding
//!   `additionalStats`, ConfigOptions.lua:2335, source `BossSkillAdditionalData`) has a random
//!   order in PoB, so each contiguous run of those mods is compared as a multiset in place.
//! * `synthetic_build_mod_list_matches_oracle` — the probe's all-vars synthetic builds
//!   (`__coc.synthetic()`: every option one at a time with every non-default value of its type
//!   including the placeholder branch, six all-vars-at-once variants, every map prefix/suffix ×
//!   tier × map mod effect, the custom modifier blocks and the `input.customMods` fallback),
//!   compared as above.
//! * `modmap_applies_match_oracle` — every `data.mapMods.AffixData[k].apply` called directly
//!   with its call shape (tiers 0-4 and 1.5, rolls 0/37/100, three effects) on a fresh pair.
//!
//! A case whose Rust run hit an `unported!` stub of another unit is reported as *pending*; a
//! stub of ConfigTab.lua / ModMap.lua fails. A missing input (data/dump, the PoB checkout,
//! .parity/builds) skips with a message; with `POB_ORACLE_REQUIRE=1` (or `CI`) it fails
//! instead. `COC_VERBOSE=1` prints every mismatch; `COC_BUILDS=<substring>` restricts the
//! corpus run.
#![allow(non_snake_case, clippy::pedantic)]

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use pob_calc::model::{Build, BuildRef};
use pob_calc::modules::mod_map::{ModMapApply, MODMAP_APPLY};
use pob_calc::prelude::*;
use pob_oracle::headless::{Engine, Ret};
use pob_oracle::jit::JitPob;
use pob_oracle::oracle::{find_parity_dir, find_pob_src, find_up, QUIET_PRELUDE};
use serde_json::Value as J;

const PROBE: &str = include_str!("../corpus/CO-C/bml_probe.lua");
const EMPTY_XML: &str = include_str!("../corpus/CO-C/empty.xml");
/// The control fields the probe records (bml_probe.lua `CONTROL_FIELDS`).
const CONTROL_FIELDS: [&str; 6] = [
    "_className",
    "selIndex",
    "placeholder",
    "state",
    "enabled",
    "buf",
];

/// The stub counters are process-global and only one PoB may live at a time (memory):
/// run the tests of this file one at a time.
static SERIAL: Mutex<()> = Mutex::new(());

fn skip(why: &str) {
    let required = std::env::var_os("POB_ORACLE_REQUIRE").is_some_and(|v| v == "1")
        || std::env::var_os("CI").is_some_and(|v| !v.is_empty() && v != "false" && v != "0");
    assert!(!required, "required input missing: {why}");
    eprintln!("SKIPPED: {why}");
}

fn dump_dir() -> Option<PathBuf> {
    if let Some(p) = std::env::var_os("POB_DUMP_DIR").map(PathBuf::from) {
        return p.join("data.bin").is_file().then_some(p);
    }
    find_up("data/dump/data.bin").map(|p| p.parent().unwrap().to_path_buf())
}

fn init_data() -> Option<&'static Data> {
    let dir = dump_dir()?;
    if !pob_gamedata::is_initialised() {
        pob_gamedata::init_from_dir(&dir).unwrap_or_else(|e| panic!("data.bin: {e}"));
    }
    Some(pob_gamedata::data())
}

/// A LuaError panic as `Err(message)` (other panics propagate).
fn catch_lua<R>(f: impl FnOnce() -> R) -> Result<R, String> {
    match std::panic::catch_unwind(std::panic::AssertUnwindSafe(f)) {
        Ok(r) => Ok(r),
        Err(p) => match p.downcast::<LuaError>() {
            Ok(e) => Err(e.0.clone()),
            Err(other) => std::panic::resume_unwind(other),
        },
    }
}

/// LuaErrors are expected (the probe sends values the UI never sends): keep them off stderr.
fn quiet_lua_errors() {
    let prev = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        if info.payload().downcast_ref::<LuaError>().is_none() {
            prev(info);
        }
    }));
}

/// A probe value (`enc` in bml_probe.lua) as a model value: tables become `Table`s
/// (`$seq` → array part, `$num` → numeric keys, `$$k` → key `$k`).
fn val_from_json(v: &J) -> Val {
    match v {
        J::Null => Val::Nil,
        J::Bool(b) => Val::Bool(*b),
        J::Number(n) => Val::Num(n.as_f64().unwrap()),
        J::String(s) => Val::from(s.as_str()),
        J::Array(a) => {
            let t = Table::new();
            for x in a {
                t.push(val_from_json(x));
            }
            Val::Table(t)
        }
        J::Object(o) => {
            if o.len() == 1 {
                if let Some(f) = o.get("$float").and_then(J::as_str) {
                    return Val::Num(match f {
                        "nan" => f64::NAN,
                        "inf" => f64::INFINITY,
                        "-inf" => f64::NEG_INFINITY,
                        "-0" => -0.0,
                        _ => panic!("probe value {v}"),
                    });
                }
            }
            let t = Table::new();
            for (k, x) in o {
                if k == "$seq" {
                    for (i, e) in x.as_array().expect("$seq").iter().enumerate() {
                        t.set(i as i64 + 1, val_from_json(e));
                    }
                } else if k == "$num" {
                    for (nk, e) in x.as_object().expect("$num") {
                        let n: f64 = nk.parse().expect("numeric key");
                        assert!(n.fract() == 0.0, "non-integer key {nk}");
                        t.set(n as i64, val_from_json(e));
                    }
                } else {
                    let k = k.strip_prefix('$').unwrap_or(k);
                    t.set(Name::from(k), val_from_json(x));
                }
            }
            Val::Table(t)
        }
    }
}

fn key_str(k: &Key) -> String {
    match k {
        Key::S(s) => format!("s:{s}"),
        Key::I(i) => format!("i:{i}"),
        Key::B(b) => format!("b:{b}"),
        Key::F(f) => format!("f:{}", f.get()),
    }
}

/// Lua value equality for the probe's values (NaN equals NaN; tables compared deeply).
fn deep_eq(a: &Val, b: &Val) -> bool {
    match (a, b) {
        (Val::Num(x), Val::Num(y)) => x == y || (x.is_nan() && y.is_nan()),
        (Val::Table(x), Val::Table(y)) => {
            let ma: BTreeMap<String, Val> = x.pairs().map(|(k, v)| (key_str(&k), v)).collect();
            let mb: BTreeMap<String, Val> = y.pairs().map(|(k, v)| (key_str(&k), v)).collect();
            ma.len() == mb.len()
                && ma
                    .iter()
                    .all(|(k, v)| mb.get(k).is_some_and(|w| deep_eq(v, w)))
        }
        _ => a.raw_eq(b),
    }
}

/// `{"mod": {…}, "marks": {…}}` → `Mod`; marks (non-standard string keys) are reported.
fn decode_mod(e: &J) -> Result<Mod, String> {
    let marks = e["marks"].as_object().expect("marks");
    if !marks.is_empty() {
        return Err(format!("lua mod has marks {e}"));
    }
    serde_json::from_value(e["mod"].clone()).map_err(|x| format!("mod {e}: {x}"))
}

fn decode_list(v: &J) -> Result<Vec<Mod>, String> {
    v.as_array()
        .expect("mod list")
        .iter()
        .map(decode_mod)
        .collect()
}

fn store_mods(ml: &ModList) -> Vec<Mod> {
    ml.ipairs().iter().map(|m| (**m).clone()).collect()
}

/// Sites hit since `before` (stub counters are global).
fn stub_hits_since(before: &[(&'static str, u64)]) -> BTreeSet<&'static str> {
    let before: BTreeMap<&str, u64> = before.iter().copied().collect();
    pob_calc::stub::stub_report()
        .into_iter()
        .filter(|(k, n)| before.get(k).map_or(true, |b| n > b))
        .map(|(k, _)| k)
        .collect()
}

/// Source of the mods `presetBossSkills` adds in `pairs(additionalStats)` order
/// (ConfigOptions.lua:2335-2340): random in PoB.
const PAIRS_ORDER_SOURCE: &str = "BossSkillAdditionalData";

/// LUA-ORDER: each contiguous run of [`PAIRS_ORDER_SOURCE`] mods sorted (compared as a
/// multiset in place); every other mod keeps its position.
fn canon_pairs_runs(mut v: Vec<Mod>) -> Vec<Mod> {
    let is_pairs = |m: &Mod| m.source.as_deref() == Some(PAIRS_ORDER_SOURCE);
    let mut i = 0;
    while i < v.len() {
        if is_pairs(&v[i]) {
            let mut j = i;
            while j < v.len() && is_pairs(&v[j]) {
                j += 1;
            }
            v[i..j].sort_by_cached_key(|m| format!("{m:?}"));
            i = j;
        } else {
            i += 1;
        }
    }
    v
}

fn mods_diff(what: &str, lua: &J, rust: Vec<Mod>) -> Option<String> {
    let rust = canon_pairs_runs(rust);
    match decode_list(lua).map(canon_pairs_runs) {
        Ok(l) if l == rust => None,
        Ok(l) => {
            let first = l
                .iter()
                .zip(&rust)
                .position(|(a, b)| a != b)
                .unwrap_or(l.len().min(rust.len()));
            Some(format!(
                "{what}: lua {} mods, rust {}; first difference at #{}:\n      lua  {:?}\n      rust {:?}",
                l.len(),
                rust.len(),
                first + 1,
                l.get(first),
                rust.get(first)
            ))
        }
        Err(e) => Some(format!("{what}: {e}")),
    }
}

// ---- probe state ---------------------------------------------------------------------------

type Map = BTreeMap<String, J>;

/// `state()` of bml_probe.lua, complete (the probe sends diffs).
#[derive(Clone, Default)]
struct State {
    input: Map,
    placeholder: Map,
    controls: Map,
    customModsList: J,
    enemyLevel: J,
    characterLevel: J,
    buildFlag: J,
}

fn obj_map(v: &J) -> Map {
    v.as_object()
        .expect("state map")
        .iter()
        .map(|(k, v)| (k.clone(), v.clone()))
        .collect()
}

/// `base` with `diff` (`diff(base, cur)` in the probe: changed keys, removed keys as null).
fn patch(base: &Map, diff: &J) -> Map {
    let mut out = base.clone();
    for (k, v) in diff.as_object().expect("state diff") {
        if v.is_null() {
            out.remove(k);
        } else {
            out.insert(k.clone(), v.clone());
        }
    }
    out
}

impl State {
    fn full(v: &J) -> Self {
        State {
            input: obj_map(&v["input"]),
            placeholder: obj_map(&v["placeholder"]),
            controls: obj_map(&v["controls"]),
            customModsList: v["customModsList"].clone(),
            enemyLevel: v["enemyLevel"].clone(),
            characterLevel: v["characterLevel"].clone(),
            buildFlag: v["buildFlag"].clone(),
        }
    }

    /// `encState(st, base)` read back against `self` (= base).
    fn apply(&self, d: &J) -> Self {
        State {
            input: patch(&self.input, &d["input"]),
            placeholder: patch(&self.placeholder, &d["placeholder"]),
            controls: patch(&self.controls, &d["controls"]),
            customModsList: d["customModsList"].clone(),
            enemyLevel: d["enemyLevel"].clone(),
            characterLevel: d["characterLevel"].clone(),
            buildFlag: d["buildFlag"].clone(),
        }
    }

    fn control(&self, var: &str, field: &str) -> Val {
        self.controls
            .get(&format!("{var}|{field}"))
            .map_or(Val::Nil, val_from_json)
    }
}

/// `varData.list` as the control's `list` (the constructor stores the last option with that
/// `var`, ConfigTab.lua:675).
fn list_table(data: &Data, var: &str) -> Option<Table> {
    let opt = data
        .configOptions
        .iter()
        .rev()
        .find(|o| o.var.is_some_and(|v| v.as_str() == var))?;
    let list = opt.list.as_ref()?;
    let t = Table::new();
    for item in list {
        let e = Table::new();
        e.set(n!("val"), Val::from_value(&item.val));
        e.set(n!("label"), &*item.label);
        t.push(e);
    }
    Some(t)
}

/// The control vars of a state (`var|field` keys).
fn control_vars(st: &State) -> BTreeSet<String> {
    st.controls
        .keys()
        .map(|k| k.split_once('|').expect("var|field").0.to_owned())
        .collect()
}

/// A model build with a control-state table per PoB control (CCR-0037: the model constructor
/// does not create them); dropdowns get their `list`.
fn new_build(data: &'static Data, vars: &BTreeSet<String>, base: &State) -> BuildRef {
    let build = Build::new(data);
    build.configTab.ctor_model();
    for var in vars {
        let c = Table::new();
        if base
            .control(var, "_className")
            .raw_eq(&Val::from("DropDownControl"))
        {
            if let Some(l) = list_table(data, var) {
                c.set(n!("list"), l);
            }
        }
        build.configTab.varControls.set(var.as_str(), c);
    }
    build
}

/// Set the probe's state into the model (the active config set: `ct.input` / `ct.placeholder`
/// alias its tables).
fn restore(build: &BuildRef, vars: &BTreeSet<String>, st: &State) {
    let ct = &build.configTab;
    for var in vars {
        let t = ct.varControls.table(var.as_str()).expect("control");
        for f in CONTROL_FIELDS {
            t.set(f, st.control(var, f));
        }
    }
    let set = ct.activeConfigSet();
    for (field, map) in [
        (n!("input"), &st.input),
        (n!("placeholder"), &st.placeholder),
    ] {
        let t = set.table(field).expect("config set table");
        t.wipe();
        for (k, v) in map {
            t.set(k.as_str(), val_from_json(v));
        }
    }
    set.set(n!("customModsList"), val_from_json(&st.customModsList));
    ct.enemyLevel.set(val_from_json(&st.enemyLevel).opt_num());
    build.buildFlag.set(val_from_json(&st.buildFlag).truthy());
    build
        .characterLevel
        .set(val_from_json(&st.characterLevel).num());
    ct.modList.set(ModStore::ModList(None));
    ct.enemyModList.set(ModStore::ModList(None));
}

/// Differences between the probe's post-state and the model's.
fn state_diffs(build: &BuildRef, vars: &BTreeSet<String>, st: &State) -> Vec<String> {
    let mut out = Vec::new();
    let ct = &build.configTab;
    let mut cmp = |what: String, l: Val, r: Val| {
        if !deep_eq(&l, &r) {
            out.push(format!("{what}: lua {l:?}, rust {r:?}"));
        }
    };
    for var in vars {
        let t = ct.varControls.table(var.as_str()).expect("control");
        for f in CONTROL_FIELDS {
            cmp(
                format!("varControls.{var}.{f}"),
                st.control(var, f),
                t.get(f),
            );
        }
    }
    let set = ct.activeConfigSet();
    for (field, map) in [
        (n!("input"), &st.input),
        (n!("placeholder"), &st.placeholder),
    ] {
        let t = set.table(field).expect("config set table");
        let mut keys: BTreeSet<String> = map.keys().cloned().collect();
        for (k, _) in t.pairs() {
            match k {
                Key::S(s) => keys.insert(s.to_string()),
                other => keys.insert(format!("<non-string key {}>", key_str(&other))),
            };
        }
        for k in keys {
            let l = map.get(&k).map_or(Val::Nil, val_from_json);
            cmp(format!("{field}.{k}"), l, t.get(k.as_str()));
        }
    }
    cmp(
        "customModsList".to_owned(),
        val_from_json(&st.customModsList),
        set.get(n!("customModsList")),
    );
    cmp(
        "enemyLevel".to_owned(),
        val_from_json(&st.enemyLevel),
        Val::from(ct.enemyLevel.get()),
    );
    cmp(
        "characterLevel".to_owned(),
        val_from_json(&st.characterLevel),
        Val::from(build.characterLevel.get()),
    );
    cmp(
        "buildFlag".to_owned(),
        val_from_json(&st.buildFlag),
        Val::Bool(build.buildFlag.get()),
    );
    out
}

// ---- comparison -----------------------------------------------------------------------------

#[derive(Default)]
struct Tally {
    cases: usize,
    matched: usize,
    both_error: usize,
    with_mods: usize,
    mods: usize,
    enemy_mods: usize,
    pending: usize,
    mismatched: usize,
    pending_sites: BTreeSet<&'static str>,
    mismatches: Vec<String>,
}

impl Tally {
    fn record(
        &mut self,
        label: String,
        lua_ok: bool,
        diffs: Vec<String>,
        hits: BTreeSet<&'static str>,
    ) {
        self.cases += 1;
        if diffs.is_empty() {
            self.matched += 1;
            if !lua_ok {
                self.both_error += 1;
            }
        } else if !hits.is_empty() {
            self.pending += 1;
            self.pending_sites.extend(hits.iter().copied());
        } else {
            self.mismatched += 1;
            if std::env::var_os("COC_VERBOSE").is_some() {
                eprintln!("  {label}: {}", diffs.join("; "));
            }
            self.mismatches
                .push(format!("{label}: {}", diffs.join("; ")));
        }
    }

    fn report(&self, what: &str) {
        eprintln!(
            "CO-C {what}: {} cases ({} with mods; {} mods, {} enemy mods in PoB): {} identical ({} both raise), {} pending (stubs: {:?}), {} mismatched",
            self.cases,
            self.with_mods,
            self.mods,
            self.enemy_mods,
            self.matched,
            self.both_error,
            self.pending,
            self.pending_sites,
            self.mismatched
        );
        for m in self.mismatches.iter().take(40) {
            eprintln!("  MISMATCH {m}");
        }
        assert!(
            self.pending_sites
                .iter()
                .all(|s| !s.starts_with("Classes/ConfigTab.lua")
                    && !s.starts_with("Data/ModMap.lua")),
            "CO-C stub hit: {:?}",
            self.pending_sites
        );
    }
}

/// One `__coc.case` record against the model: restore its pre-state, run BuildModList, compare.
fn check_case(build: &BuildRef, vars: &BTreeSet<String>, base: &State, c: &J, t: &mut Tally) {
    let name = c["name"].as_str().expect("case name").to_owned();
    let pre = base.apply(&c["pre"]);
    let post = pre.apply(&c["post"]);
    restore(build, vars, &pre);
    let before = pob_calc::stub::stub_report();
    let r = catch_lua(|| build.configTab.BuildModList(build));
    let hits = stub_hits_since(&before);
    let lua_ok = c["ok"].as_bool().expect("ok");
    let n_mods = c["mods"].as_array().map_or(0, Vec::len);
    let n_enemy = c["enemy"].as_array().map_or(0, Vec::len);
    t.mods += n_mods;
    t.enemy_mods += n_enemy;
    if n_mods + n_enemy > 0 {
        t.with_mods += 1;
    }
    let mut diffs = Vec::new();
    if lua_ok != r.is_ok() {
        diffs.push(format!(
            "status: lua {} ({}), rust {:?}",
            lua_ok,
            c["err"].as_str().unwrap_or(""),
            r
        ));
    }
    diffs.extend(mods_diff(
        "modList",
        &c["mods"],
        store_mods(&build.configTab.modList.get()),
    ));
    diffs.extend(mods_diff(
        "enemyModList",
        &c["enemy"],
        store_mods(&build.configTab.enemyModList.get()),
    ));
    diffs.extend(state_diffs(build, vars, &post));
    t.record(name, lua_ok, diffs, hits);
}

fn boot(src: &Path) -> JitPob {
    let mut pob =
        JitPob::boot_oracle(src, Some(QUIET_PRELUDE)).unwrap_or_else(|e| panic!("PoB boot: {e}"));
    pob.load_build_xml(EMPTY_XML, "coc_empty")
        .unwrap_or_else(|e| panic!("load: {e}"));
    pob.run(PROBE)
        .unwrap_or_else(|e| panic!("bml_probe.lua: {e}"));
    pob
}

fn first_str(pob: &mut JitPob, code: &str) -> String {
    match pob
        .run(code)
        .unwrap_or_else(|e| panic!("{code}: {e}"))
        .first()
    {
        Some(Ret::Str(s)) => s.clone(),
        other => panic!("{code} returned {other:?}"),
    }
}

/// TP1: the texts PoB returns for `calls` (run in order after `boot`, then, with `build`, after
/// loading that build), from the shared oracle query cache (`testkit::oracle_query`) or a fresh
/// LuaJIT PoB under the oracle lock. The key holds the probe, the empty build, the calls and the
/// build's name and XML.
fn pob_texts(src: &Path, before: &[&str], build: Option<(&str, &str)>, after: &[&str]) -> Vec<String> {
    let (name, xml) = build.unwrap_or(("", ""));
    let mut key = vec!["CO-C pob_texts 1", QUIET_PRELUDE, EMPTY_XML, PROBE, name, "--before"];
    key.extend_from_slice(before);
    key.push("--after");
    key.extend_from_slice(after);
    let text = pob_oracle::testkit::oracle_query(src, xml.as_bytes(), &key, || {
        let mut pob = boot(src);
        let mut out: Vec<String> = before.iter().map(|c| first_str(&mut pob, c)).collect();
        if let Some((name, xml)) = build {
            pob.load_build_xml(xml, name)
                .unwrap_or_else(|e| panic!("{name}: {e}"));
        }
        out.extend(after.iter().map(|c| first_str(&mut pob, c)));
        Ok(J::from(out).to_string())
    })
    .unwrap_or_else(|e| panic!("CO-C oracle query: {e}"));
    serde_json::from_str(&text).unwrap_or_else(|e| panic!("cached CO-C query: {e}"))
}

fn parse(s: &str) -> J {
    serde_json::from_str(s).unwrap_or_else(|e| panic!("probe json: {e}"))
}

// ---- tests ----------------------------------------------------------------------------------

#[test]
fn corpus_build_mod_list_matches_oracle() {
    let _serial = SERIAL
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let Some(data) = init_data() else {
        return skip("data/dump/data.bin not found");
    };
    let Some(src) = find_pob_src() else {
        return skip("PoB checkout (.PathOfBuilding/src) not found");
    };
    let Some(parity) = find_parity_dir() else {
        return skip(".parity not found");
    };
    let mut builds: Vec<PathBuf> = std::fs::read_dir(parity.join("builds"))
        .map(|rd| rd.filter_map(|e| e.ok().map(|e| e.path())).collect())
        .unwrap_or_default();
    builds.retain(|p| p.extension().is_some_and(|e| e == "xml"));
    if let Ok(f) = std::env::var("COC_BUILDS") {
        builds.retain(|p| p.to_string_lossy().contains(&f));
    }
    builds.sort();
    if builds.is_empty() {
        return skip(".parity/builds/*.xml not found");
    }
    quiet_lua_errors();
    let mut t = Tally::default();
    let mut custom = 0usize;
    let mut model: Option<(BuildRef, BTreeSet<String>, State)> = None;
    // TP1: PoB's side of every build on the bounded pool (`$POB_TEST_PAR`; each one PoB, cached),
    // the Rust side here in corpus order (it reads the process-wide `unported!` counters)
    let pob_side = |b: &PathBuf| {
        let stem = b.file_stem().unwrap().to_string_lossy().into_owned();
        let xml = std::fs::read_to_string(b).unwrap();
        let case_call = format!("return __coc.case({stem:?})");
        let mut t = pob_texts(
            &src,
            &["return __coc.baseline()"],
            Some((&stem, &xml)),
            &[&case_call],
        );
        let case = t.pop().expect("case");
        let base = t.pop().expect("baseline");
        (base, case)
    };
    let mut each = |base: String, case: String| {
        let base = State::full(&parse(&base));
        let case = parse(&case);
        if case["pre"]["customModsList"].as_array().is_some_and(|l| {
            l.iter()
                .any(|b| b["text"].as_str().is_some_and(|s| !s.is_empty()))
        }) {
            custom += 1;
        }
        let vars = control_vars(&base);
        if model.as_ref().map_or(true, |(_, v, _)| *v != vars) {
            model = Some((new_build(data, &vars, &base), vars, base.clone()));
        }
        let (build, vars, _) = model.as_ref().unwrap();
        check_case(build, vars, &base, &case, &mut t);
    };
    pob_oracle::testkit::par_for_each(&builds, |_, b| pob_side(b), |_, _, (base, case)| {
        each(base, case);
    });
    drop(each);
    eprintln!(
        "CO-C corpus: {} builds ({} with custom modifier text)",
        builds.len(),
        custom
    );
    eprintln!("{}", pob_oracle::runner::query_cache_summary());
    t.report("corpus BuildModList");
    assert_eq!(t.mismatched, 0, "corpus BuildModList differs from PoB");
}

#[test]
fn synthetic_build_mod_list_matches_oracle() {
    let _serial = SERIAL
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let Some(data) = init_data() else {
        return skip("data/dump/data.bin not found");
    };
    let Some(src) = find_pob_src() else {
        return skip("PoB checkout (.PathOfBuilding/src) not found");
    };
    quiet_lua_errors();
    let (base, doc) = {
        let mut t = pob_texts(
            &src,
            &["return __coc.baseline()", "return __coc.synthetic()"],
            None,
            &[],
        );
        let doc = t.pop().expect("synthetic");
        (t.pop().expect("baseline"), doc)
    };
    let base = State::full(&parse(&base));
    let doc = parse(&doc);
    let cases = doc["cases"].as_array().expect("cases");
    let vars = control_vars(&base);
    let build = new_build(data, &vars, &base);
    let mut t = Tally::default();
    let mut groups: BTreeMap<&str, usize> = BTreeMap::new();
    for c in cases {
        let name = c["name"].as_str().unwrap_or("");
        let g = if name.starts_with("opt") {
            "single option"
        } else if name.starts_with("allVars") {
            "all vars"
        } else if name.starts_with("map ") {
            "map affix"
        } else {
            "custom mods"
        };
        *groups.entry(g).or_default() += 1;
        check_case(&build, &vars, &base, c, &mut t);
    }
    eprintln!("CO-C synthetic groups: {groups:?}");
    t.report("synthetic BuildModList");
    assert!(
        groups.get("all vars") == Some(&6),
        "all-vars variants: {groups:?}"
    );
    assert!(t.with_mods > 1000, "too few synthetic cases produce mods");
    assert_eq!(t.mismatched, 0, "synthetic BuildModList differs from PoB");
}

#[test]
fn modmap_applies_match_oracle() {
    let _serial = SERIAL
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let Some(data) = init_data() else {
        return skip("data/dump/data.bin not found");
    };
    let Some(src) = find_pob_src() else {
        return skip("PoB checkout (.PathOfBuilding/src) not found");
    };
    quiet_lua_errors();
    let doc = pob_texts(&src, &["return __coc.modmap()"], None, &[])
        .pop()
        .expect("modmap");
    let doc = parse(&doc);
    let mut t = Tally::default();
    let mut keys = BTreeSet::new();
    for c in doc["cases"].as_array().expect("cases") {
        let k = c["key"].as_str().expect("key");
        keys.insert(k.to_owned());
        let a = data
            .mapMods
            .AffixData
            .get(&Name::from(k))
            .unwrap_or_else(|| panic!("AffixData.{k} missing from data.bin"));
        let id = &a.apply.as_ref().expect("probed affix has apply").id;
        let f = MODMAP_APPLY
            .get(id.as_str())
            .unwrap_or_else(|| panic!("{id} not in MODMAP_APPLY"))
            .f;
        let values = Table::new();
        for v in &a.values {
            values.push(Val::from_value(v));
        }
        let effect = val_from_json(&c["effect"]).num();
        let tier = val_from_json(&c["tier"]);
        let roll = val_from_json(&c["roll"]);
        let ml = ModStore::ModList(None);
        let eml = ModStore::ModList(None);
        let before = pob_calc::stub::stub_report();
        let r = catch_lua(|| match f {
            ModMapApply::check(f) => f(&Val::from(k), effect, &ml, &eml),
            ModMapApply::list(f) => f(tier.num(), effect, &values, &ml, &eml),
            ModMapApply::count(f) => f(tier.num(), roll.num(), effect, &values, &ml, &eml),
        });
        let hits = stub_hits_since(&before);
        let lua_ok = c["ok"].as_bool().expect("ok");
        let n =
            c["mods"].as_array().map_or(0, Vec::len) + c["enemy"].as_array().map_or(0, Vec::len);
        t.mods += c["mods"].as_array().map_or(0, Vec::len);
        t.enemy_mods += c["enemy"].as_array().map_or(0, Vec::len);
        if n > 0 {
            t.with_mods += 1;
        }
        let mut diffs = Vec::new();
        if lua_ok != r.is_ok() {
            diffs.push(format!(
                "status: lua {} ({}), rust {:?}",
                lua_ok,
                c["err"].as_str().unwrap_or(""),
                r
            ));
        }
        diffs.extend(mods_diff("modList", &c["mods"], store_mods(&ml)));
        diffs.extend(mods_diff("enemyModList", &c["enemy"], store_mods(&eml)));
        let label = format!(
            "AffixData.{k} ({}) tier={} roll={} effect={}",
            c["type"], c["tier"], c["roll"], c["effect"]
        );
        t.record(label, lua_ok, diffs, hits);
    }
    let with_apply: BTreeSet<String> = data
        .mapMods
        .AffixData
        .iter()
        .filter(|(_, a)| a.apply.is_some())
        .map(|(k, _)| k.as_str().to_owned())
        .collect();
    eprintln!("CO-C modmap: {} affixes with apply probed", keys.len());
    t.report("ModMap applies");
    assert_eq!(
        keys, with_apply,
        "affixes PoB probed vs data.bin affixes with apply"
    );
    assert_eq!(t.mismatched, 0, "ModMap applies differ from PoB");
}
