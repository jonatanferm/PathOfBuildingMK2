//! Unit CS3 acceptance (calc doc §11.1 row CS3, §9.4 L4/L5): CalcSetup.lua 1451-1975, the
//! skills segment of `calcs.initEnv` (`calc_setup::init_env_skills::seg`).
//!
//! **L4 (`cs3_l4_skills_segment`).** Every `initEnv:1451#n` hit is injected, the Rust segment
//! runs, and the state is diffed against the `initEnv:end` hit of the same env (CalcSetup.lua:1974,
//! the line before `return env, …`; the patcher's function probe `initEnv:out` fires after the
//! return, i.e. the same state). A hit whose env never reaches 1974 is PoB's Energy Blade restart
//! (1923): the Rust segment must return `Flow::Restart` with `AffectedByEnergyBlade` appended to
//! `override.conditions`.
//!
//! **The build model.** CS3 reads and rewrites `build.skillsTab.socketGroupList` (and reads
//! `build.mainSocketGroup`, `build.itemsTab.slots[*].weaponSet`,
//! `build.itemsTab.activeItemSet.useSecondWeaponSet`, `skillsTab.imbuedSupportBySlot`,
//! `skillsTab.displayGroup`). A pob-state dump writes the build as an `$object` marker, and the
//! build loader (BM-A) is not ported, so the test dumps the model itself: PoB is patched with
//! `corpus/CS3/stages.toml` (the two line probes) and the probes' local table gets one more entry,
//! `cs3Model` (`corpus/CS3/model.lua`, a Lua expression over `build`). The model's socket groups
//! and gem instances are therefore part of the same identity graph as the env (`activeSkill.socketGroup`,
//! `effect.srcInstance` are `$ref`s to them). The test rebuilds them as `SocketGroup` /
//! `GemInstance` objects of a fresh build model before the run, and exports the model again after
//! it as the `cs3Model` local, so `diff_against` compares env, locals and model in one tree.
//!
//! **Dumps.** One `pob_oracle probe` run per build (both probes, every hit, same process, so in
//! and out pair up) into `$CARGO_TARGET_TMPDIR/CS3/<stem>/`, reused while complete.
//!
//! **Representation differences** (counted and printed, not gated):
//! * `nil-default`: a typed Rust field cannot be nil, so a key PoB leaves nil exports as its
//!   default (`false`, `0`, `""`, empty table, NaN for the CCR-0019 nil numbers) or vice versa;
//! * `model-marker`: the `$labelNil` / `$includeInFullDPSNil` keys of CCR-0019 on the groups CS3
//!   creates (PoB: the field is nil);
//! * `value-identity`: PoB shares one table (a GemProperty row's `value` is the mod's own value
//!   table) where the Rust exporter writes an immutable data value in full; the tables are equal;
//! * `LUA-ORDER (mod store)` (the diff's own order class), `LUA-ORDER (buffList)` (AS: buff
//!   order follows the stat-map `pairs` order), `LUA-ORDER (supportLists pairs, 1807/1827)` (a
//!   skill whose group has a `source`, or whose slot receives cross-linked supports, collects
//!   supports in `pairs` order over group-keyed tables: PoB's address order. Only for those
//!   skills, and only when our `supportList`/`effectList` holds the same granted effects as
//!   PoB's, our list is reordered to PoB's before the diff, pairing effects by granted-effect id,
//!   and refs into it are rewritten; each effect is then diffed against its PoB counterpart.
//!   Counted per reordered list);
//! * `gem variant (CCR-0010)`: mods of a skill whose gem resolved to its `AltX`/`AltY` variant in
//!   one of the two PoB processes (the oracle's, the data dump's);
//! * `minion`: the minion's fresh `modDB` and the minion item set, which is not in the dump
//!   (BM-A). (`minion.uses` / `lifeTable` are typed since RETYPE and compared exactly.)
//!
//! Before diffing, `warcryPowerBonus` is dropped from PoB's tree (CalcPerform.lua:2382 writes it
//! into a shared mod's tag in an earlier perform; CCR-0083 §7).
//!
//! **Gate.** No hit raises, every hit that ran without reaching an `unported!` stub has zero
//! unexplained mismatches, every restart hit restarts, and at least one hit passes. Hits whose
//! run reached a stub are reported as stub-blocked with the sites.
//!
//! **L5 (`cs3_l5_init_env`).** `calcs.initEnv(build, mode)` from the build XML for all57 × MAIN /
//! CALCS / CALCULATOR against the `initEnv:end` dump of that mode's first loaded env. The model
//! is BM-A's (`build_xml::loadBuildModelFromXML`), with the part of PoB's first post-load
//! `CalcsTab:BuildOutput()` that precedes the target run first ([`calcs_tab_prefix`]); the
//! timeless LUTs load from the PoB checkout. Gate: no run raises, every stub-free run has zero
//! unexplained mismatches. L5 compares the env only: on top of L4's classes, [`classify_l5`]
//! explains what the dump's scope and the export leave unequal (the cleared `cs3Model`, other
//! envs' objects, the opaque `nodesInRadius` maps (compared from the env), jewel func values,
//! node tags, item nil-defaults). FS1 made it run: it loaded the model with the pre-BM-A
//! `testkit::load_build` (no tree, every run raised) and compared CALCS / CALCULATOR against a
//! model without the MAIN pass.
//!
//! Inputs: `data/dump`, the PoB checkout, `.parity/builds` (missing fails unless
//! `POB_ALLOW_SKIP=1`). Knobs: `CS3_BUILDS=stem1,stem2`, `CS3_VERBOSE=1` (every mismatch),
//! `CS3_HITS=all|sample` (default `sample`: per build the first loaded hit of each mode/specEnv
//! shape, the main env's hit, every MAIN hit after the main env up to 2, and every restart).
#![allow(non_snake_case)]
#![allow(clippy::too_many_lines)]

use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::OnceLock;

use pob_calc::flow::Flow;
use pob_calc::model::{BuildRef, GemInstance, GemInstanceRef, SocketGroup, SocketGroupRef};
use pob_calc::probe::{
    mark_absent, rc_ptr, set_member, typed_keys, DataRef, DataTarget, Kind, PKey, PObj,
};
use pob_calc::{Table, Val};
use pob_lua::Name;
use pob_oracle::diff::{diff_trees, tree_eq_unordered, DiffOptions, Mismatch, MismatchKind};
use pob_oracle::schema::{children, resolve_ids, StateDump};
use pob_oracle::testkit;
use serde_json::Value as J;

const MODEL_LUA: &str = include_str!("../corpus/CS3/model.lua");

/// The two tests share the dumps directory and the process-wide stub counters: run them one at
/// a time.
fn serial() -> std::sync::MutexGuard<'static, ()> {
    static M: std::sync::Mutex<()> = std::sync::Mutex::new(());
    M.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
}

fn corpus_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("corpus/CS3")
}

/// `$CARGO_TARGET_TMPDIR/CS3/<hash of the probe inputs>`: new probes or a new model expression
/// get fresh patched sources and dumps.
fn tmp_dir() -> PathBuf {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    MODEL_LUA.hash(&mut h);
    std::fs::read_to_string(corpus_dir().join("stages.toml"))
        .unwrap_or_default()
        .hash(&mut h);
    Path::new(env!("CARGO_TARGET_TMPDIR"))
        .join("CS3")
        .join(format!("{:016x}", h.finish()))
}

fn stem(p: &Path) -> String {
    p.file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or_default()
        .to_owned()
}

/// all57 (`.parity/builds/*.xml`), filtered by `CS3_BUILDS`.
fn builds() -> Option<Vec<PathBuf>> {
    let dir = pob_testpaths::require(pob_testpaths::parity_dir(), ".parity")?.join("builds");
    let mut v: Vec<PathBuf> = std::fs::read_dir(&dir)
        .ok()?
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|x| x == "xml"))
        .collect();
    v.sort();
    if let Ok(list) = std::env::var("CS3_BUILDS") {
        let want: Vec<&str> = list.split(',').map(str::trim).collect();
        v.retain(|p| want.contains(&stem(p).as_str()));
    }
    Some(v)
}

/// The patched PoB source (the two line probes plus the `cs3Model` local), made once.
fn patched_src() -> Result<PathBuf, String> {
    static P: OnceLock<Result<PathBuf, String>> = OnceLock::new();
    P.get_or_init(|| {
        let src = pob_testpaths::pob_src().ok_or("no PoB checkout")?;
        let root = tmp_dir().join("patched");
        std::fs::create_dir_all(&root).map_err(|e| e.to_string())?;
        let out = std::process::Command::new(env!("CARGO_BIN_EXE_pob_oracle"))
            .arg("patch")
            .arg("--stages")
            .arg(corpus_dir().join("stages.toml"))
            .arg("--pob")
            .arg(&src)
            .arg("--root")
            .arg(&root)
            .stderr(std::process::Stdio::null())
            .output()
            .map_err(|e| format!("pob_oracle patch: {e}"))?;
        if !out.status.success() {
            return Err(format!("pob_oracle patch failed ({})", out.status));
        }
        let dir = String::from_utf8_lossy(&out.stdout)
            .lines()
            .last()
            .unwrap_or_default()
            .trim()
            .to_owned();
        let dir = PathBuf::from(dir);
        let f = dir.join("Modules/CalcSetup.lua");
        let text = std::fs::read_to_string(&f).map_err(|e| format!("{}: {e}", f.display()))?;
        let expr = MODEL_LUA.trim_end().to_owned() + " ";
        if !text.contains("cs3Model = ") {
            let mut t = text.clone();
            for id in ["initEnv:1451", "initEnv:end"] {
                let anchor = format!("__PROBE(\"{id}\", {{ build = build, ");
                if t.matches(&anchor).count() != 1 {
                    return Err(format!("patched CalcSetup.lua: probe {id} not found once"));
                }
                t = t.replace(&anchor, &format!("{anchor}{expr}"));
            }
            std::fs::write(&f, t).map_err(|e| e.to_string())?;
        }
        Ok(dir)
    })
    .clone()
}

/// The build's dumps from one `pob_oracle probe` run (made on first use).
fn ensure_dumps(xml: &Path, stem: &str) -> Result<PathBuf, String> {
    let dir = tmp_dir().join("dumps").join(stem);
    let state = dir.join("state");
    let done = dir.join("complete");
    if done.is_file() {
        return Ok(dir);
    }
    let src = patched_src()?;
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    let status = std::process::Command::new(env!("CARGO_BIN_EXE_pob_oracle"))
        .arg("probe")
        .arg(xml)
        .arg("--out-dir")
        .arg(&dir)
        .arg("--patched")
        .arg(&src)
        .arg("--select")
        .arg("initEnv:1451,initEnv:end")
        .arg("--max-dumps")
        .arg("400")
        .arg("--no-provenance")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map_err(|e| format!("pob_oracle probe: {e}"))?;
    if !status.success() || !state.is_dir() {
        return Err(format!(
            "pob_oracle probe {} failed ({status})",
            xml.display()
        ));
    }
    std::fs::write(&done, b"").map_err(|e| format!("{}: {e}", done.display()))?;
    Ok(dir)
}

/// The dump files of a probe in `state/`, by hit number.
fn hit_files(state: &Path, probe: &str) -> BTreeMap<u64, PathBuf> {
    let mut out = BTreeMap::new();
    let Ok(rd) = std::fs::read_dir(state) else {
        return out;
    };
    for e in rd.filter_map(Result::ok) {
        let name = e.file_name().to_string_lossy().into_owned();
        let base = name
            .strip_suffix(".json.zst")
            .or_else(|| name.strip_suffix(".json"))
            .unwrap_or("");
        let Some((p, n)) = base.split_once('#') else {
            continue;
        };
        if p == probe {
            if let Ok(n) = n.parse::<u64>() {
                out.insert(n, e.path());
            }
        }
    }
    out
}

/// `{"$env": id}` and the shape of a dump without loading it twice.
struct HitInfo {
    n: u64,
    env_id: String,
    mode: String,
    spec_env: bool,
    loaded: bool,
}

fn hit_info(d: &StateDump) -> HitInfo {
    let loc = d.root.get("locals");
    let model_groups = loc
        .and_then(|l| l.get("cs3Model"))
        .and_then(|m| m.get("socketGroupList"))
        .is_some_and(|g| g.as_array().is_some_and(|a| !a.is_empty()));
    HitInfo {
        n: d.n,
        env_id: d.env_id.clone().unwrap_or_default(),
        mode: loc
            .and_then(|l| l.get("mode"))
            .and_then(J::as_str)
            .unwrap_or("")
            .to_owned(),
        spec_env: loc
            .and_then(|l| l.get("specEnv"))
            .is_some_and(|s| !s.is_null()),
        loaded: model_groups,
    }
}

/// Give the model's socket groups a typed context in the dump, so that injection decodes them
/// (and through `gemList`, `displayEffect`, `srcInstance`, `isSupporting` everything below them)
/// as `SocketGroup` / `GemInstance` / `SkillEffect` objects with one identity each: a local
/// `cs3GroupKinds` of stand-in active skills (`$uuid` makes a table an active skill for
/// ORC-X's kind pass) whose `socketGroup` refers to each group. Inline group tables get an `$id`.
fn type_model(dump: &mut StateDump) {
    fn max_id(v: &J, m: &mut u64) {
        match v {
            J::Object(o) => {
                if let Some(n) = o.get("$id").and_then(J::as_u64) {
                    *m = (*m).max(n);
                }
                o.values().for_each(|x| max_id(x, m));
            }
            J::Array(a) => a.iter().for_each(|x| max_id(x, m)),
            _ => {}
        }
    }
    let mut next = 0u64;
    max_id(&dump.root, &mut next);
    let Some(J::Object(locals)) = dump.root.get_mut("locals") else {
        return;
    };
    let mut ids = Vec::new();
    let mut take = |g: &mut J, ids: &mut Vec<u64>| {
        if let J::Object(o) = g {
            if let Some(n) = o.get("$ref").and_then(J::as_u64) {
                ids.push(n);
            } else if let Some(n) = o.get("$id").and_then(J::as_u64) {
                ids.push(n);
            } else if !pob_oracle::schema::is_marker(o) {
                next += 1;
                o.insert("$id".to_owned(), J::from(next));
                ids.push(next);
            }
        }
    };
    if let Some(J::Object(model)) = locals.get_mut("cs3Model") {
        if let Some(J::Array(list)) = model.get_mut("socketGroupList") {
            for g in list.iter_mut() {
                take(g, &mut ids);
            }
        }
        if let Some(g) = model.get_mut("displayGroup") {
            take(g, &mut ids);
        }
    }
    let kinds: Vec<J> = ids
        .into_iter()
        .map(|n| serde_json::json!({ "$uuid": "cs3-model", "socketGroup": { "$ref": n } }))
        .collect();
    locals.insert("cs3GroupKinds".to_owned(), J::Array(kinds));
}

fn panic_msg(p: &(dyn std::any::Any + Send)) -> String {
    p.downcast_ref::<pob_lua::LuaError>()
        .map(|e| e.0.clone())
        .or_else(|| p.downcast_ref::<String>().cloned())
        .or_else(|| p.downcast_ref::<&str>().map(|s| (*s).to_owned()))
        .unwrap_or_else(|| "panic".to_owned())
}

// ---- the build model from the dumped `cs3Model` -------------------------------------------------

#[derive(Default)]
struct ModelConv {
    groups: HashMap<usize, SocketGroupRef>,
    gems: HashMap<usize, GemInstanceRef>,
}

impl ModelConv {
    fn gem(&mut self, v: &Val) -> Result<GemInstanceRef, String> {
        if let Some(g) = v.obj::<GemInstance>() {
            return Ok(g);
        }
        let t = v
            .table()
            .ok_or_else(|| format!("gem instance is a {} value", v.lua_type()))?;
        let key = t.as_ptr() as usize;
        if let Some(g) = self.gems.get(&key) {
            return Ok(g.clone());
        }
        // The same Lua table reached through a typed context (`effect.srcInstance` of the
        // gem's own `displayEffect` / `supportEffect`, set together at CalcSetup.lua
        // 1695-1696 / 1787) was injected as a `GemInstance` already: use that object, so the
        // gem keeps one identity (ORC-X decodes the dynamic occurrence as a separate table).
        for k in ["displayEffect", "supportEffect"] {
            if let Some(e) = t
                .get(Name::from(k))
                .obj::<pob_calc::active_skill::SkillEffect>()
            {
                if let Some(src) = e.srcInstance.get() {
                    self.gems.insert(key, src.clone());
                    return Ok(src);
                }
            }
        }
        let g: GemInstanceRef = Rc::new(GemInstance::default());
        self.gems.insert(key, g.clone());
        let o = PObj::Gem(g.clone());
        let mut seen = Vec::new();
        for (k, v) in t.pairs() {
            if let Some(name) = k.name() {
                seen.push(name.as_str().to_owned());
                set_member(&o, &PKey::S(name.as_str().to_owned()), v);
            }
        }
        for k in typed_keys(Kind::Gem) {
            if !seen.iter().any(|s| s == k) {
                mark_absent(rc_ptr(&g), k);
            }
        }
        Ok(g)
    }

    fn group(&mut self, v: &Val) -> Result<SocketGroupRef, String> {
        if let Some(g) = v.obj::<SocketGroup>() {
            // nil model numbers are NaN (CCR-0019 / CCR-0083 §3); injection left the default
            let p = rc_ptr(&g);
            for (k, c) in [
                ("mainActiveSkill", &g.mainActiveSkill),
                ("mainActiveSkillCalcs", &g.mainActiveSkillCalcs),
            ] {
                if pob_calc::probe::with_registry(|r| r.is_absent(p, k)) {
                    c.set(f64::NAN);
                }
            }
            return Ok(g);
        }
        let t = v
            .table()
            .ok_or_else(|| format!("socket group is a {} value", v.lua_type()))?;
        let key = t.as_ptr() as usize;
        if let Some(g) = self.groups.get(&key) {
            return Ok(g.clone());
        }
        let g: SocketGroupRef = Rc::new(SocketGroup::default());
        self.groups.insert(key, g.clone());
        let o = PObj::Group(g.clone());
        let mut seen = Vec::new();
        for (k, v) in t.pairs() {
            let Some(name) = k.name() else { continue };
            seen.push(name.as_str().to_owned());
            match name.as_str() {
                "gemList" => {
                    let list = v.table().ok_or("socketGroup.gemList is not a table")?;
                    let mut gems = Vec::new();
                    for (_, x) in list.ipairs() {
                        gems.push(self.gem(&x)?);
                    }
                    g.gemList.set(pob_lua::Shared::new(gems));
                }
                "displayGemList" => {
                    let list = v
                        .table()
                        .ok_or("socketGroup.displayGemList is not a table")?;
                    let out = Table::new();
                    for (_, x) in list.ipairs() {
                        out.push(Val::Obj(self.gem(&x)?));
                    }
                    g.extra.set(name, out);
                }
                k => {
                    set_member(&o, &PKey::S(k.to_owned()), v);
                }
            }
        }
        // nil model numbers are NaN (CCR-0019 / CCR-0083 §3)
        if !seen.iter().any(|s| s == "mainActiveSkill") {
            g.mainActiveSkill.set(f64::NAN);
        }
        if !seen.iter().any(|s| s == "mainActiveSkillCalcs") {
            g.mainActiveSkillCalcs.set(f64::NAN);
        }
        if !seen.iter().any(|s| s == "label") {
            pob_calc::model::skills_tab::set_socketGroup_label(&g, None);
        }
        for k in typed_keys(Kind::Group) {
            if !seen.iter().any(|s| s == k) {
                mark_absent(rc_ptr(&g), k);
            }
        }
        Ok(g)
    }
}

/// Fill the build model from the injected `cs3Model` local.
fn install_model(build: &BuildRef, model: &Table) -> Result<(), String> {
    let mut conv = ModelConv::default();
    let list = model
        .table(Name::from("socketGroupList"))
        .ok_or("cs3Model.socketGroupList missing")?;
    let mut groups = Vec::new();
    for (_, v) in list.ipairs() {
        groups.push(conv.group(&v)?);
    }
    build
        .skillsTab
        .socketGroupList
        .set(pob_lua::Shared::new(groups));
    let dg = model.get(Name::from("displayGroup"));
    build.skillsTab.displayGroup.set(if dg.is_nil() {
        None
    } else {
        Some(conv.group(&dg)?)
    });
    build.mainSocketGroup.set(
        model
            .opt_num(Name::from("mainSocketGroup"))
            .unwrap_or(f64::NAN),
    );
    // read by the active-skill code CS3 calls (CalcActiveSkill.lua 675, 718)
    if let Some(l) = model.opt_num(Name::from("characterLevel")) {
        build.characterLevel.set(l);
    }
    if let Some(list) = model.table(Name::from("spectreList")) {
        let names: Vec<Name> = list
            .ipairs()
            .into_iter()
            .filter_map(|(_, v)| v.str().map(|s| Name::from(&*s)))
            .collect();
        build.spectreList.set(pob_lua::Shared::new(names));
    }
    if let Some(ws) = model.table(Name::from("weaponSets")) {
        build.itemsTab.slots.with_mut(|m| m.clear());
        for (k, v) in ws.pairs() {
            if let Some(name) = k.name() {
                let slot = Table::new();
                slot.set(Name::from("weaponSet"), v);
                build.itemsTab.slots.with_mut(|m| {
                    m.insert(name, slot);
                });
            }
        }
    }
    let set = Table::new();
    set.set(
        Name::from("useSecondWeaponSet"),
        model.get(Name::from("useSecondWeaponSet")),
    );
    build.itemsTab.activeItemSet.set(Some(set));
    Ok(())
}

/// `item.sockets` of the env's items (not part of a pob-state item dump): from `cs3Model.itemSockets`.
fn install_sockets(env: &pob_calc::env::EnvRef, model: &Table) {
    let Some(socks) = model.table(Name::from("itemSockets")) else {
        return;
    };
    for (k, v) in socks.pairs() {
        let (Some(slot), Some(list)) = (k.name(), v.table()) else {
            continue;
        };
        let item = env.player.item_in(slot);
        if let Some(item) = item {
            let sockets: Vec<Table> = list
                .ipairs()
                .into_iter()
                .filter_map(|(_, x)| x.table())
                .collect();
            item.sockets.set(pob_lua::Shared::new(sockets));
        }
    }
}

fn install_imbued(build: &BuildRef, model: &Table) -> Result<(), String> {
    if let Some(im) = model.table(Name::from("imbuedSupportBySlot")) {
        for (k, v) in im.pairs() {
            let ge = v.obj::<DataRef>().and_then(|d| match &d.target {
                DataTarget::Skill(ge) => Some(ge.clone()),
                _ => None,
            });
            match ge {
                Some(ge) => build
                    .skillsTab
                    .imbuedSupportBySlot
                    .set(k, Val::Obj(Rc::new(ge))),
                None => build.skillsTab.imbuedSupportBySlot.set(k, v),
            }
        }
    }
    Ok(())
}

/// The model after the run, as the `cs3Model` local PoB dumps at 1974.
fn export_model(build: &BuildRef, before: &Table) -> Table {
    let t = Table::new();
    let list = Table::new();
    for g in build.skillsTab.socketGroupList.get().snapshot() {
        list.push(Val::Obj(g));
    }
    t.set(Name::from("socketGroupList"), list);
    let msg = build.mainSocketGroup.get();
    if !msg.is_nan() {
        t.set(Name::from("mainSocketGroup"), msg);
    }
    t.set(
        Name::from("weaponSets"),
        before.get(Name::from("weaponSets")),
    );
    t.set(
        Name::from("useSecondWeaponSet"),
        before.get(Name::from("useSecondWeaponSet")),
    );
    t.set(
        Name::from("imbuedSupportBySlot"),
        before.get(Name::from("imbuedSupportBySlot")),
    );
    if let Some(g) = build.skillsTab.displayGroup.get() {
        t.set(Name::from("displayGroup"), Val::Obj(g));
    }
    t.set(
        Name::from("itemSockets"),
        before.get(Name::from("itemSockets")),
    );
    t.set(
        Name::from("characterLevel"),
        before.get(Name::from("characterLevel")),
    );
    t.set(
        Name::from("spectreList"),
        before.get(Name::from("spectreList")),
    );
    t
}

// ---- classification -------------------------------------------------------------------------------

fn json_is_default(v: &J) -> bool {
    match v {
        J::Null => true,
        J::Bool(b) => !b,
        J::Number(n) => n.as_f64() == Some(0.0),
        J::String(s) => s.is_empty(),
        J::Array(a) => a.is_empty(),
        J::Object(o) => {
            o.is_empty()
                || o.get("$num").and_then(J::as_str) == Some("nan")
                || o.get("$float").and_then(J::as_str) == Some("nan")
        }
    }
}

/// `key` is a typed field of one of the probe's struct kinds (env, actor, skill, effect, cfg,
/// gem, group, override): the kinds whose fields cannot hold nil; or `SpecEnv::accelerate`
/// (the `specEnv` local, a typed struct without a probe kind: probe.rs `impl ProbeField for
/// SpecEnv` always writes it).
fn typed_field(path: &str, key: &str) -> bool {
    path.ends_with("specEnv.accelerate")
        || [
            Kind::Env,
            Kind::Actor,
            Kind::Skill,
            Kind::Effect,
            Kind::Cfg,
            Kind::Gem,
            Kind::Group,
            Kind::Override,
        ]
        .into_iter()
        .any(|k| typed_keys(k).contains(&key))
}

/// The value at an encoded path (`env.player.modDB`, `x[3]`, `x[@2:key]`) of a resolved tree.
fn lookup<'a>(root: &'a J, path: &str) -> Option<&'a J> {
    let mut cur = root;
    let mut base = String::new();
    loop {
        if base == path {
            return Some(cur);
        }
        let mut next = None;
        for (p, c) in children(&base, cur) {
            let hit = path == p
                || (path.starts_with(p.as_str())
                    && matches!(path.as_bytes().get(p.len()), Some(b'.' | b'[' | b'<')));
            if hit {
                next = Some((p, c));
                break;
            }
        }
        let (p, c) = next?;
        base = p;
        cur = c;
    }
}

/// `testkit::diff_against`, keeping the two resolved trees for the classification, with our
/// support lists aligned first ([`align_permuted_supports`]; the count of reordered lists is the
/// last member).
fn diff_with_trees(
    dump: &StateDump,
    env: Option<&pob_calc::env::EnvRef>,
    locals: &[(String, Val)],
    stem: &str,
) -> (Vec<Mismatch>, J, J, usize) {
    let pob_locals: Vec<String> = dump
        .locals()
        .and_then(J::as_object)
        .map(|m| m.keys().cloned().collect())
        .unwrap_or_default();
    let kept: Vec<(String, Val)> = locals
        .iter()
        .filter(|(k, _)| pob_locals.contains(k))
        .cloned()
        .collect();
    let ours = testkit::export_after(env, &kept);
    let mut pob = dump.root.clone();
    if let Some(J::Object(l)) = pob.get_mut("locals") {
        let ours_names: Vec<&String> = kept.iter().map(|(k, _)| k).collect();
        l.retain(|k, v| {
            ours_names.contains(&k) || !v.as_object().is_some_and(|m| m.contains_key("$fn"))
        });
    }
    let nondet = pob_testpaths::parity_dir()
        .map(|p| p.join("nondet").join(format!("{stem}.json")))
        .filter(|p| p.is_file())
        .and_then(|p| pob_oracle::nondet::NondetRecord::load(&p).ok());
    let opts = DiffOptions {
        nondet_values: nondet.as_ref().and_then(|n| n.states.get(&dump.hit)),
        nondet_orders: nondet.as_ref().and_then(|n| n.state_orders.get(&dump.hit)),
        ..DiffOptions::strict()
    };
    // CalcPerform.lua:2382 writes `warcryPowerBonus` into a warcry buff mod's first tag in place;
    // the mod is shared with later envs (stat-map mods are cached, CalcActiveSkill.lua:21), so
    // PoB's dump carries an earlier perform's value. CS3 never writes it: drop the key.
    fn strip(v: &mut J) {
        match v {
            J::Object(o) => {
                o.remove("warcryPowerBonus");
                o.values_mut().for_each(strip);
            }
            J::Array(a) => a.iter_mut().for_each(strip),
            _ => {}
        }
    }
    strip(&mut pob);
    // FS1: ours too. An injected mod keeps the key PoB's earlier perform wrote (L4 injects the
    // `initEnv:1451` dump; seen on `env.override.extraJewelFuncs.actor.modDB`, an actor PoB's
    // initEnv:1451 dump already holds), so stripping one side only reported it as a mod mismatch.
    let mut ours = ours;
    strip(&mut ours);
    let (pob, mut ours) = (resolve_ids(&pob), resolve_ids(&ours));
    let reordered = align_permuted_supports(&pob, &mut ours);
    (diff_trees(&pob, &ours, &opts), pob, ours, reordered)
}

/// `{"$ref": path}` → path.
fn ref_path(v: &J) -> Option<&str> {
    let o = v.as_object()?;
    if o.len() == 1 {
        o.get("$ref").and_then(J::as_str)
    } else {
        None
    }
}

/// `path` is `p` or lies below it.
fn path_under(path: &str, p: &str) -> bool {
    path == p
        || (path.starts_with(p) && matches!(path.as_bytes().get(p.len()), Some(b'.' | b'[' | b'<')))
}

/// The mutable value at an encoded path (string members, sequence / `$seq` elements and `$num`
/// keys; the steps a path to a skill's effect list takes).
fn lookup_mut<'a>(root: &'a mut J, path: &str) -> Option<&'a mut J> {
    use pob_oracle::schema::{is_marker, step_index, step_key, unescape_key};
    let mut cur = root;
    let mut base = String::new();
    loop {
        if base == path {
            return Some(cur);
        }
        let (p, next) = match cur {
            J::Array(a) => a
                .iter_mut()
                .enumerate()
                .map(|(i, x)| (step_index(&base, &(i + 1).to_string()), x))
                .find(|(p, _)| path_under(path, p))?,
            J::Object(m) if !is_marker(m) => {
                let key = m.keys().find_map(|k| {
                    let p = step_key(&base, unescape_key(k)?);
                    path_under(path, &p).then(|| (k.clone(), p))
                });
                let in_seq = matches!(m.get("$seq"), Some(J::Array(_)));
                if let Some((k, p)) = key {
                    (p, m.get_mut(&k)?)
                } else if in_seq {
                    let Some(J::Array(seq)) = m.get_mut("$seq") else {
                        return None;
                    };
                    seq.iter_mut()
                        .enumerate()
                        .map(|(i, x)| (step_index(&base, &(i + 1).to_string()), x))
                        .find(|(p, _)| path_under(path, p))?
                } else if let Some(J::Object(num)) = m.get_mut("$num") {
                    num.iter_mut()
                        .map(|(k, x)| (step_index(&base, k), x))
                        .find(|(p, _)| path_under(path, p))?
                } else {
                    return None;
                }
            }
            _ => return None,
        };
        base = p;
        cur = next;
    }
}

/// The granted-effect ids of an encoded effect list (`supportList` / `effectList`), refs followed.
fn effect_ids(root: &J, list: &J) -> Vec<String> {
    children("", list)
        .into_iter()
        .map(|(_, e)| {
            let e = ref_path(e).and_then(|p| lookup(root, p)).unwrap_or(e);
            e.get("grantedEffect").map(J::to_string).unwrap_or_default()
        })
        .collect()
}

/// `perm[j]` = the index in `ours` of PoB's `j`-th id (the k-th occurrence of an id pairs with
/// the k-th occurrence), or `None` when the two are not the same multiset.
fn id_permutation(pob: &[String], ours: &[String]) -> Option<Vec<usize>> {
    if pob.len() != ours.len() {
        return None;
    }
    let mut used = vec![false; ours.len()];
    pob.iter()
        .map(|id| {
            let i = (0..ours.len()).find(|&i| !used[i] && ours[i] == *id)?;
            used[i] = true;
            Some(i)
        })
        .collect()
}

/// `LUA-ORDER (supportLists pairs, 1807/1827)`. A skill whose socket group has a `source` (item /
/// tree granted, CalcSetup.lua:1795) gathers the slot's supports through
/// `pairs(supportLists[slotName])` (1807), and one in a cross-link target slot through
/// `pairs(supportLists[crossLinkedSupportSlot])` (1827): group-keyed tables, so PoB's order is its
/// address order (per process; calc doc §3.4). Every other group keeps the deterministic order of
/// `supportLists[group]` (1791) and is compared as is.
///
/// For each such skill (decided on PoB's side) whose `supportList` / `effectList` holds the same
/// granted effects as PoB's in another order, our list is reordered to PoB's, pairing the
/// effects by granted-effect id, and every ref into the moved elements is rewritten, so the diff
/// that follows compares each effect with its PoB counterpart in full (level, quality, flags,
/// gem config). Returns the number of lists reordered.
fn align_permuted_supports(pob: &J, ours: &mut J) -> usize {
    fn rewrite(v: &mut J, moves: &[(String, String)]) {
        match v {
            J::Object(m) => {
                if m.len() == 1 {
                    if let Some(J::String(r)) = m.get_mut("$ref") {
                        if let Some((o, n)) = moves.iter().find(|(o, _)| path_under(r, o)) {
                            *r = format!("{n}{}", &r[o.len()..]);
                        }
                        return;
                    }
                }
                m.values_mut().for_each(|x| rewrite(x, moves));
            }
            J::Array(a) => a.iter_mut().for_each(|x| rewrite(x, moves)),
            _ => {}
        }
    }
    let follow = |root: &J, p: &str| -> Option<String> {
        let v = lookup(root, p)?;
        Some(ref_path(v).map_or_else(|| p.to_owned(), str::to_owned))
    };
    // the slots that receive cross-linked supports (1596-1597), PoB's side
    let cross_targets: Vec<String> = lookup(pob, "env.crossLinkedSupportGroups")
        .map(|t| {
            children("", t)
                .into_iter()
                .flat_map(|(_, list)| children("", list))
                .filter_map(|(_, s)| s.as_str().map(str::to_owned))
                .collect()
        })
        .unwrap_or_default();
    let mut cands: Vec<String> = vec!["env.player.mainSkill".to_owned()];
    for list in ["env.player.activeSkillList", "env.auxSkillList"] {
        if let Some(J::Array(a)) = lookup(pob, list) {
            for i in 1..=a.len() {
                cands.push(format!("{list}[{i}]"));
            }
        }
    }
    let mut skills: Vec<String> = Vec::new();
    for c in cands {
        let (Some(sp), Some(so)) = (follow(pob, &c), follow(ours, &c)) else {
            continue;
        };
        if sp != so || skills.contains(&sp) {
            continue;
        }
        let Some(skill) = lookup(pob, &sp) else {
            continue;
        };
        let group = skill
            .get("socketGroup")
            .map(|g| ref_path(g).and_then(|p| lookup(pob, p)).unwrap_or(g));
        let sourced = group
            .and_then(|g| g.get("source"))
            .is_some_and(|s| !s.is_null());
        let cross = group
            .and_then(|g| g.get("slot"))
            .and_then(J::as_str)
            .is_some_and(|s| {
                let s = s.trim_end_matches(" Swap");
                cross_targets.iter().any(|t| t == s)
            });
        if sourced || cross {
            skills.push(sp);
        }
    }
    // (old path, new path) of every moved element
    let mut moves: Vec<(String, String)> = Vec::new();
    let mut reordered = 0;
    for s in &skills {
        for list in ["supportList", "effectList"] {
            let lp = format!("{s}.{list}");
            let (Some(la), Some(lb)) = (lookup(pob, &lp), lookup(ours, &lp)) else {
                continue;
            };
            let (ia, ib) = (effect_ids(pob, la), effect_ids(ours, lb));
            if ia == ib {
                continue;
            }
            let Some(perm) = id_permutation(&ia, &ib) else {
                continue;
            };
            let Some(node) = lookup_mut(ours, &lp) else {
                continue;
            };
            let seq = match node {
                J::Array(a) => a,
                J::Object(m) => match m.get_mut("$seq") {
                    Some(J::Array(a)) => a,
                    _ => continue,
                },
                _ => continue,
            };
            let old = std::mem::take(seq);
            *seq = perm.iter().map(|&i| old[i].clone()).collect();
            for (j, &i) in perm.iter().enumerate() {
                if i != j {
                    moves.push((format!("{lp}[{}]", i + 1), format!("{lp}[{}]", j + 1)));
                }
            }
            reordered += 1;
        }
    }
    if !moves.is_empty() {
        rewrite(ours, &moves);
    }
    reordered
}

/// `Skill:<id>` with a CCR-0010 `AltX`/`AltY` suffix dropped.
fn strip_alt(source: &str) -> String {
    source
        .strip_suffix("AltX")
        .or_else(|| source.strip_suffix("AltY"))
        .unwrap_or(source)
        .to_owned()
}

/// The prefix of `path` up to and including the segment `seg` (`….buffList[1].modList`).
fn prefix_through<'a>(path: &'a str, seg: &str) -> Option<&'a str> {
    let i = path.find(seg)?;
    Some(&path[..i + seg.len()])
}

/// (unexplained, explained category counts).
fn classify(
    mm: Vec<Mismatch>,
    pob: &J,
    ours: &J,
    reordered: usize,
) -> (Vec<Mismatch>, BTreeMap<&'static str, usize>) {
    let mut rest = Vec::new();
    let mut why: BTreeMap<&'static str, usize> = BTreeMap::new();
    if reordered > 0 {
        why.insert("LUA-ORDER (supportLists pairs, 1807/1827)", reordered);
    }
    let strict = DiffOptions::strict();
    let mod_source = |v: &Option<J>| -> Option<String> {
        v.as_ref()?.get("source")?.as_str().map(str::to_owned)
    };
    // CCR-0010: skills whose gem resolved to an `AltX`/`AltY` variant in one of the two PoB
    // processes (the oracle's, the one the data dump came from)
    let alt_bases: Vec<String> = mm
        .iter()
        .filter_map(|m| mod_source(&m.pob).or_else(|| mod_source(&m.ours)))
        .filter(|s| s.ends_with("AltX") || s.ends_with("AltY"))
        .map(|s| strip_alt(&s))
        .collect();
    for m in mm {
        if matches!(m.kind, MismatchKind::ModMissing | MismatchKind::ModExtra) {
            if let Some(src) = mod_source(&m.pob).or_else(|| mod_source(&m.ours)) {
                if alt_bases.contains(&strip_alt(&src)) {
                    *why.entry("gem variant (CCR-0010)").or_default() += 1;
                    continue;
                }
            }
        }
        // the minion's fresh modDB, and the minion item set (`build.itemsTab.itemSets`, not in
        // the dump; BM-A)
        if let Some(i) = m.path.find(".minion.") {
            let field = &m.path[i + ".minion.".len()..];
            let ok = match field.split(['.', '[']).next().unwrap_or("") {
                "modDB" => {
                    matches!(m.kind, MismatchKind::Extra)
                        && m.ours
                            .as_ref()
                            .and_then(|o| o.get("mods"))
                            .is_some_and(|x| x.as_object().is_some_and(serde_json::Map::is_empty))
                }
                "itemSet" | "weaponData1" | "weaponData2" => true,
                _ => false,
            };
            if ok {
                *why.entry("minion (AS representation / item sets not dumped)")
                    .or_default() += 1;
                continue;
            }
        }
        if m.path.ends_with(".skillMinionItemSet") || m.path.ends_with(".skillMinionItemSetCalcs") {
            *why.entry("minion (AS representation / item sets not dumped)")
                .or_default() += 1;
            continue;
        }
        if matches!(m.kind, MismatchKind::LuaOrder) {
            *why.entry("LUA-ORDER (mod store)").or_default() += 1;
            continue;
        }
        // PoB shares one table where the Rust export writes an immutable data value in full
        // (`row.value` of the Tabulate rows is the mod's own value table, CalcSetup.lua 1614)
        // (a ref on either side is followed once; the referenced tables must be equal)
        if let (Some(a), Some(b)) = (&m.pob, &m.ours) {
            let follow = |v: &'_ J, root: &'_ J| -> Option<J> {
                match v.as_object() {
                    Some(o) if o.len() == 1 => match o.get("$ref").and_then(J::as_str) {
                        Some(p) => lookup(root, p).cloned(),
                        None => Some(v.clone()),
                    },
                    _ => Some(v.clone()),
                }
            };
            let is_ref = |v: &J| {
                v.as_object()
                    .is_some_and(|o| o.len() == 1 && o.contains_key("$ref"))
            };
            if matches!(m.kind, MismatchKind::Value) && (is_ref(a) || is_ref(b)) {
                if let (Some(x), Some(y)) = (follow(a, pob), follow(b, ours)) {
                    if !is_ref(&x) && !is_ref(&y) && diff_trees(&x, &y, &strict).is_empty() {
                        *why.entry("value-identity").or_default() += 1;
                        continue;
                    }
                }
            }
        }
        // buff lists in PoB's skill-mod-list order (stat-map `pairs`, AS's LUA-ORDER)
        if let Some(pre) = prefix_through(&m.path, ".buffList") {
            if let (Some(a), Some(b)) = (lookup(pob, pre), lookup(ours, pre)) {
                if tree_eq_unordered(a, b, strict.rel) {
                    *why.entry("LUA-ORDER (buffList)").or_default() += 1;
                    continue;
                }
            }
        }
        let last = m.path.rsplit('.').next().unwrap_or("");
        if last.contains("labelNil") || last.contains("includeInFullDPSNil") {
            *why.entry("model-marker").or_default() += 1;
            continue;
        }
        // only a typed Rust field (a probe struct's KEYS) has a default where PoB has nil; a key
        // of an untyped table (an effect's `extra`, a mod's tag) must match as is
        let default_side = typed_field(&m.path, last)
            && match m.kind {
                MismatchKind::Extra => m.ours.as_ref().is_some_and(json_is_default),
                MismatchKind::Missing => m.pob.as_ref().is_some_and(json_is_default),
                _ => false,
            };
        if default_side {
            if std::env::var_os("CS3_SHOW_EXPLAINED").is_some() {
                eprintln!("    (nil-default) {}", fmt_mismatch(&m));
            }
            *why.entry("nil-default").or_default() += 1;
            continue;
        }
        rest.push(m);
    }
    (rest, why)
}

fn fmt_mismatch(m: &Mismatch) -> String {
    let short = |j: &Option<J>| {
        let s = j.as_ref().map(J::to_string).unwrap_or_else(|| "-".into());
        if s.len() > 200 {
            format!("{}…", &s[..200])
        } else {
            s
        }
    };
    format!(
        "{} {:?}: pob={} ours={}",
        m.path,
        m.kind,
        short(&m.pob),
        short(&m.ours)
    )
}

// ---- one hit ----------------------------------------------------------------------------------------

enum Outcome {
    Pass(BTreeMap<&'static str, usize>),
    Restart,
    Fail(Vec<String>, BTreeMap<&'static str, usize>),
    StubBlocked(Vec<String>, usize),
    Error(String),
}

fn run_hit(din: &StateDump, dout: Option<&StateDump>, stem: &str) -> Outcome {
    let build = testkit::empty_build();
    let mut din_typed = din.clone();
    type_model(&mut din_typed);
    let inj = match pob_oracle::inject::inject(&din_typed, &build) {
        Ok(i) => i,
        Err(e) => return Outcome::Error(format!("inject {}: {e}", din.hit)),
    };
    let locals: Vec<(String, Val)> = inj.locals.into_iter().collect();
    let Some(model) = locals
        .iter()
        .find(|(k, _)| k == "cs3Model")
        .and_then(|(_, v)| v.table())
    else {
        return Outcome::Error(format!("{}: no cs3Model local", din.hit));
    };
    if let Err(e) = install_model(&build, &model).and_then(|()| install_imbued(&build, &model)) {
        return Outcome::Error(format!("{}: model: {e}", din.hit));
    }
    if let Some(env) = inj.env.as_ref() {
        install_sockets(env, &model);
    }
    pob_calc::stub::stub_reset();
    let run = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        pob_calc::probe::run_segment(&din.probe, &locals)
    }));
    let stubs: Vec<String> = pob_calc::stub::stub_report()
        .iter()
        .map(|(site, n)| format!("{site} x{n}"))
        .collect();
    let (flow, mut after) = match run {
        Ok(Ok(x)) => x,
        Ok(Err(e)) => return Outcome::Error(format!("run {}: {e}", din.hit)),
        Err(p) => {
            if !stubs.is_empty() {
                return Outcome::StubBlocked(stubs, 1);
            }
            return Outcome::Error(format!("run {}: {}", din.hit, panic_msg(&*p)));
        }
    };
    let Some(dout) = dout else {
        // PoB restarted initEnv (1923): no `initEnv:end` for this env.
        let ov = after
            .iter()
            .find(|(k, _)| k == "override")
            .and_then(|(_, v)| v.obj::<pob_calc::env::Override>());
        let cond = ov.and_then(|o| o.conditions.get()).is_some_and(|c| {
            c.snapshot()
                .iter()
                .any(|n| n.as_str() == "AffectedByEnergyBlade")
        });
        return if flow == Flow::Restart && cond {
            Outcome::Restart
        } else {
            Outcome::Fail(
                vec![format!(
                    "PoB restarted initEnv, Rust returned {flow:?} (conditions appended: {cond})"
                )],
                BTreeMap::new(),
            )
        };
    };
    if flow != Flow::Normal {
        return Outcome::Fail(
            vec![format!("Rust returned {flow:?}, PoB reached 1974")],
            BTreeMap::new(),
        );
    }
    let env = after
        .iter()
        .find(|(k, _)| k == "env")
        .and_then(|(_, v)| v.obj::<pob_calc::env::Env>())
        .or(inj.env);
    after.retain(|(k, _)| k != "cs3Model");
    after.push((
        "cs3Model".to_owned(),
        Val::Table(export_model(&build, &model)),
    ));
    if let Ok(f) = std::env::var("CS3_DUMP_OURS") {
        let j = testkit::export_after(env.as_ref(), &after);
        let _ = std::fs::write(
            format!("{f}.{}.json", din.n),
            serde_json::to_string(&j).unwrap_or_default(),
        );
    }
    let (mm, pob_tree, ours_tree, reordered) = diff_with_trees(dout, env.as_ref(), &after, stem);
    let (rest, why) = classify(mm, &pob_tree, &ours_tree, reordered);
    let lines: Vec<String> = rest.iter().map(fmt_mismatch).collect();
    match (lines.is_empty(), stubs.is_empty()) {
        (true, true) => Outcome::Pass(why),
        (false, true) => Outcome::Fail(lines, why),
        (_, false) => {
            if std::env::var_os("CS3_VERBOSE").is_some() {
                for l in &lines {
                    eprintln!("    (stub-blocked) {l}");
                }
            }
            Outcome::StubBlocked(stubs, lines.len())
        }
    }
}

/// Which `initEnv:1451` hits of a build to run (see the module docs).
fn select_hits(infos: &[HitInfo], main_env: Option<&str>, restarts: &[u64]) -> Vec<u64> {
    if std::env::var("CS3_HITS").is_ok_and(|v| v == "all") {
        return infos.iter().map(|h| h.n).collect();
    }
    let mut want = Vec::new();
    let mut shapes: Vec<(String, bool)> = Vec::new();
    let mut after_main = 0;
    let mut seen_main = false;
    for h in infos {
        let shape = (h.mode.clone(), h.spec_env);
        let is_main = main_env == Some(h.env_id.as_str());
        if h.loaded && !shapes.contains(&shape) {
            shapes.push(shape);
            want.push(h.n);
        } else if is_main || restarts.contains(&h.n) {
            want.push(h.n);
        } else if seen_main && h.mode == "MAIN" && after_main < 2 {
            after_main += 1;
            want.push(h.n);
        }
        if is_main {
            seen_main = true;
        }
    }
    want.sort_unstable();
    want.dedup();
    want
}

#[derive(Default)]
struct Tally {
    pass: usize,
    restart: usize,
    fail: usize,
    blocked: usize,
    errors: usize,
    explained: BTreeMap<&'static str, usize>,
    blocked_sites: BTreeMap<String, usize>,
    fail_paths: BTreeMap<String, usize>,
}

/// A mismatch path with list indices and ids generalised (for the summary).
fn path_shape(p: &str) -> String {
    let mut out = String::new();
    let mut in_br = false;
    for c in p.chars() {
        match c {
            '[' => {
                in_br = true;
                out.push_str("[]");
            }
            ']' => in_br = false,
            _ if in_br => {}
            _ => out.push(c),
        }
    }
    out
}

#[test]
fn cs3_l4_skills_segment() {
    let _serial = serial();
    if pob_testpaths::require(pob_testpaths::dump_dir(), "data/dump").is_none() {
        return;
    }
    assert!(testkit::init_default_data(), "data/dump does not load");
    let Some(builds) = builds() else { return };
    let verbose = std::env::var_os("CS3_VERBOSE").is_some();
    let mut t = Tally::default();
    let mut lines = Vec::new();
    let mut missing = Vec::new();
    for xml in &builds {
        // PoB runs one process per build: the CalcSetup.lua:1637 `fromItem` writes (CCR-0083 §1)
        // last for that build only
        pob_calc::modules::calc_setup::init_env_skills::reset_grantedEffect_fromItem();
        let stem = stem(xml);
        let dir = match ensure_dumps(xml, &stem) {
            Ok(d) => d,
            Err(e) => {
                missing.push(format!("{stem}: {e}"));
                continue;
            }
        };
        let state = dir.join("state");
        let main_env: Option<String> = std::fs::read_to_string(dir.join("hits.json"))
            .ok()
            .and_then(|s| serde_json::from_str::<J>(&s).ok())
            .and_then(|j| j.get("mainEnv").and_then(J::as_str).map(str::to_owned));
        let ins = hit_files(&state, "initEnv:1451");
        let outs = hit_files(&state, "initEnv:end");
        // env id -> out file
        let mut out_by_env: HashMap<String, PathBuf> = HashMap::new();
        for p in outs.values() {
            if let Ok(d) = StateDump::load(p) {
                if let Some(e) = d.env_id.clone() {
                    out_by_env.insert(e, p.clone());
                }
            }
        }
        let mut infos = Vec::new();
        let mut restarts = Vec::new();
        for p in ins.values() {
            if let Ok(d) = StateDump::load(p) {
                let h = hit_info(&d);
                if !out_by_env.contains_key(&h.env_id) {
                    restarts.push(h.n);
                }
                infos.push(h);
            }
        }
        let want = select_hits(&infos, main_env.as_deref(), &restarts);
        for n in want {
            let Some(pin) = ins.get(&n) else { continue };
            let Ok(din) = StateDump::load(pin) else {
                t.errors += 1;
                lines.push(format!("ERROR   {stem} initEnv:1451#{n}: unreadable"));
                continue;
            };
            let env_id = din.env_id.clone().unwrap_or_default();
            let dout = out_by_env
                .get(&env_id)
                .and_then(|p| StateDump::load(p).ok());
            let tag = format!(
                "{stem} {} ({}{}{})",
                din.hit,
                hit_info(&din).mode,
                if hit_info(&din).spec_env {
                    ", specEnv"
                } else {
                    ""
                },
                if main_env.as_deref() == Some(env_id.as_str()) {
                    ", main"
                } else {
                    ""
                }
            );
            match run_hit(&din, dout.as_ref(), &stem) {
                Outcome::Pass(why) => {
                    t.pass += 1;
                    for (k, v) in &why {
                        *t.explained.entry(k).or_default() += v;
                    }
                    lines.push(format!("ok      {tag} {why:?}"));
                }
                Outcome::Restart => {
                    t.restart += 1;
                    lines.push(format!(
                        "restart {tag}: Flow::Restart, AffectedByEnergyBlade"
                    ));
                }
                Outcome::Fail(mm, why) => {
                    t.fail += 1;
                    for (k, v) in &why {
                        *t.explained.entry(k).or_default() += v;
                    }
                    lines.push(format!("FAIL    {tag}: {} mismatches {why:?}", mm.len()));
                    for l in &mm {
                        *t.fail_paths
                            .entry(path_shape(l.split(' ').next().unwrap_or("")))
                            .or_default() += 1;
                    }
                    let show = if verbose { mm.len() } else { 12 };
                    for l in mm.iter().take(show) {
                        lines.push(format!("          {l}"));
                    }
                }
                Outcome::StubBlocked(sites, n) => {
                    t.blocked += 1;
                    lines.push(format!(
                        "BLOCKED {tag} ({n} mismatches): {}",
                        sites.join(", ")
                    ));
                    for s in sites {
                        *t.blocked_sites
                            .entry(s.split(" x").next().unwrap_or("").to_owned())
                            .or_default() += 1;
                    }
                }
                Outcome::Error(e) => {
                    t.errors += 1;
                    lines.push(format!("ERROR   {tag}: {e}"));
                }
            }
        }
    }
    for l in &lines {
        eprintln!("{l}");
    }
    eprintln!(
        "CS3 L4 initEnv:1451 -> initEnv:end: pass {} / restart {} / fail {} / stub-blocked {} / errors {}; {} builds without dumps",
        t.pass,
        t.restart,
        t.fail,
        t.blocked,
        t.errors,
        missing.len()
    );
    eprintln!("  explained representation differences: {:?}", t.explained);
    for (s, n) in &t.blocked_sites {
        eprintln!("  stub {s}: blocked {n} hits");
    }
    let mut fp: Vec<(&String, &usize)> = t.fail_paths.iter().collect();
    fp.sort_by(|a, b| b.1.cmp(a.1));
    for (p, n) in fp.iter().take(40) {
        eprintln!("  mismatch path {p}: {n}");
    }
    for m in &missing {
        eprintln!("missing: {m}");
    }
    if !missing.is_empty() && pob_testpaths::require::<()>(None, &missing.join("; ")).is_none() {
        return;
    }
    assert_eq!(t.errors, 0, "CS3 L4: {} hits raised (see stderr)", t.errors);
    assert_eq!(
        t.fail, 0,
        "CS3 L4: {} stub-free hits mismatch (see stderr)",
        t.fail
    );
    assert!(t.pass > 0, "CS3 L4: no hit ran stub-free");
}

// ---- L5 ---------------------------------------------------------------------------------------------

/// What PoB's `Build:Init` has run on the model before the env L5 compares (FS1). The targets are
/// the first loaded env of each mode (MAIN: `calcsTab.mainEnv`), all of the first post-load
/// `CalcsTab:BuildOutput()` (CalcsTab.lua 441-472): MAIN is its first initEnv; CALCS follows the
/// MAIN `calcs.buildOutput` (whose initEnv writes the socket groups' `displayLabel` /
/// `displaySkillList`, the gem instances' `displayEffect`, … into the model the CALCS env reads);
/// CALCULATOR (`getMiscCalculator`, L470) follows both. Running the same prefix here gives the
/// model PoB's initEnv saw; the build loads fresh for each mode.
fn calcs_tab_prefix(build: &BuildRef, mode: pob_calc::env::Mode) {
    use pob_calc::env::Mode;
    use pob_calc::modules::calcs::buildOutput;
    if mode == Mode::MAIN {
        return;
    }
    // L442-456
    build.calcsTab.powerBuildFlag.set(true);
    let spec = build.spec.get();
    for (_, node) in spec.nodes.with(|n| n.pairs()) {
        node.finalModList.set(Some(node.modList.get()));
    }
    // L459-460
    let mainEnv = buildOutput(build, Mode::MAIN);
    build
        .calcsTab
        .mainOutput
        .set(Some(mainEnv.player.output.get()));
    build.calcsTab.mainEnv.set(Some(mainEnv));
    if mode == Mode::CALCS {
        return;
    }
    // L461-462
    let calcsEnv = buildOutput(build, Mode::CALCS);
    build
        .calcsTab
        .calcsOutput
        .set(Some(calcsEnv.player.output.get()));
    build.calcsTab.calcsEnv.set(Some(calcsEnv));
}

/// Break the build ↔ env cycle of an L5 run (`env.build` is a strong `BuildRef` and the prefix
/// stores envs in `calcsTab`), so each build is freed (~20 MB a build otherwise, as INT-3 notes).
fn release(env: &pob_calc::env::EnvRef) {
    let b = &env.build;
    b.calcsTab.mainEnv.set(None);
    b.calcsTab.calcsEnv.set(None);
    b.calcsTab.mainOutput.set(None);
    b.calcsTab.calcsOutput.set(None);
}

/// `{"$fn": …}`: a Lua function in PoB's dump.
fn is_lua_fn(v: &J) -> bool {
    v.as_object()
        .is_some_and(|o| o.len() == 1 && o.contains_key("$fn"))
}

/// `{"func": {"$fn": key}, "type": …}`: the Rust `Value::JewelFunc` (`JewelFuncSpec`) export.
fn is_jewel_func_spec(v: &J) -> bool {
    v.as_object().is_some_and(|o| {
        o.get("func").is_some_and(is_lua_fn) && o.keys().all(|k| k == "func" || k == "type")
    })
}

/// `{"$foreign": …}` / `{"$env": …}`: an object of another env in PoB's dump.
fn is_foreign(v: &J) -> bool {
    v.as_object()
        .is_some_and(|o| o.len() == 1 && (o.contains_key("$foreign") || o.contains_key("$env")))
}

/// `v` with every `func` member that is a Lua function (PoB) or a Rust jewel func spec (ours)
/// replaced by one placeholder, so that two mods whose values differ only there compare equal
/// (the value's own `type` stays compared).
fn norm_jewel_funcs(v: &J) -> J {
    match v {
        J::Object(o) => J::Object(
            o.iter()
                .map(|(k, x)| {
                    if k == "func" && (is_lua_fn(x) || is_jewel_func_spec(x)) {
                        (k.clone(), J::String("$jewelFunc".to_owned()))
                    } else {
                        (k.clone(), norm_jewel_funcs(x))
                    }
                })
                .collect(),
        ),
        J::Array(a) => J::Array(a.iter().map(norm_jewel_funcs).collect()),
        _ => v.clone(),
    }
}

/// The encoded index of `path` right after `prefix` (`env.radiusJewelList[3].nodes[…]` → 3).
fn index_after(path: &str, prefix: &str) -> Option<usize> {
    let rest = path.strip_prefix(prefix)?.strip_prefix('[')?;
    rest[..rest.find(']')?].parse().ok()
}

/// L5-only representation classes (FS1), on top of [`classify`]. Each is a difference of the
/// export or of the dump's scope, checked against the Rust env where the export cannot show it:
/// * `model (cleared cs3Model)`: PoB's value is a `$ref` into the dump's `cs3Model` local
///   (`?N`: the model's socket groups and gem instances, which L5 drops; L4 compares them),
///   while ours exports the same model object inline or by its first env path;
/// * `nodesInRadius (opaque export)`: `radiusJewelList[i].nodes` is the tree's
///   `Rc<LjIntMap<NodeRef>>` (CCR-0024 §3), exported as `{"$userdata": true}`; the node ids are
///   compared with PoB's here, from the env;
/// * `jewel func (Value::JewelFunc)`: `radiusJewelList[i].func` is PoB's closure (`$fn`) and
///   ours the `JewelFuncSpec` data value (`node_mods.rs`, CCR-0039 §4); the entry's `type` is
///   compared on its own path;
/// * `node tag (spec node without a tree overlay)`: PoB tags a node `$node` by its identity in
///   `spec.nodes`, the Rust exporter by `node.tree` (probe.rs); a spec-only node (cluster jewel
///   sockets and subgraph nodes, which PoB's spec also owns) exports as `$treeNode`. Accepted
///   in `allocNodes` only when ours is the env spec's node object of that id, elsewhere
///   (`explodeSources`, a gem's `explodeSource`) when the spec's node of that id has no overlay;
/// * `gem variant (CCR-0010)`: values equal up to an `AltX`/`AltY` gem id (calc doc §9.5 names
///   upstream_Dual_Wield_Cospris_CoC's `VaalRighteousFire`), and that gem instance's `nameSpec`;
/// * `nil-default (item field)`: a typed `Item` bool PoB leaves nil (`corrupted`), exported
///   `false`;
/// * `port plumbing`: `override["$extraJewelFuncsActor"]` (Rust-only key of the override that
///   holds the `extraJewelFuncs` list's actor), and the list's `actor` exported as a ref to it;
///   in the global cache, a copied skill's `actorKeep` (CCR-0113) and that kept actor's
///   `enemy`, an `ActorWeak` of a finished pass whose env is gone;
/// * `foreign env object`: PoB writes another env's object as `$foreign`/`$env` (the global
///   cache's skills and envs, `displaySkillList` entries of other passes); ours exports it inline
///   or, reached first through the global cache, as a ref there.
fn classify_l5(
    mm: Vec<Mismatch>,
    pob: &J,
    ours: &J,
    env: &pob_calc::env::EnvRef,
) -> (Vec<Mismatch>, BTreeMap<&'static str, usize>) {
    let mut rest = Vec::new();
    let mut why: BTreeMap<&'static str, usize> = BTreeMap::new();
    let is_model_ref = |v: &Option<J>| {
        v.as_ref()
            .and_then(ref_path)
            .is_some_and(|p| p.starts_with('?'))
    };
    let radius_ok = |i: usize| -> bool {
        let base = format!("env.radiusJewelList[{i}].nodes");
        let Some(mut pn) = lookup(pob, &base) else {
            return false;
        };
        // two entries of one jewel share the socket's map (`nodesInRadius[radiusIndex]`)
        if let Some(p) = ref_path(pn) {
            let Some(t) = lookup(pob, p) else {
                return false;
            };
            pn = t;
        }
        let pob_ids: std::collections::BTreeSet<i64> = pn
            .get("$num")
            .and_then(J::as_object)
            .map(|m| m.keys().filter_map(|k| k.parse().ok()).collect())
            .unwrap_or_default();
        let ours_ids = env.radiusJewelList.get().with(|v| {
            v.get(i - 1).and_then(|e| match e.get(Name::from("nodes")) {
                Val::Obj(o) => o
                    .downcast_ref::<pob_lua::LjIntMap<pob_calc::classes::NodeRef>>()
                    .map(|m| {
                        m.keys()
                            .into_iter()
                            .collect::<std::collections::BTreeSet<i64>>()
                    }),
                _ => None,
            })
        });
        ours_ids.is_some_and(|o| o == pob_ids)
    };
    let mut radius_checked: BTreeMap<usize, bool> = BTreeMap::new();
    let pairs: Vec<(String, MismatchKind, J)> = mm
        .iter()
        .filter(|m| matches!(m.kind, MismatchKind::ModMissing | MismatchKind::ModExtra))
        .filter_map(|m| {
            let v = m.pob.as_ref().or(m.ours.as_ref())?;
            Some((m.path.clone(), m.kind, norm_jewel_funcs(v)))
        })
        .collect();
    for m in mm {
        if is_model_ref(&m.pob) && matches!(m.kind, MismatchKind::Value) {
            *why.entry("model (cleared cs3Model)").or_default() += 1;
            continue;
        }
        if let Some(i) = index_after(&m.path, "env.radiusJewelList") {
            let nodes = format!("env.radiusJewelList[{i}].nodes");
            let opaque = lookup(ours, &nodes)
                .is_some_and(|v| v.get("$userdata").is_some_and(|b| b == &J::Bool(true)));
            if opaque
                && m.path.starts_with(&nodes)
                && *radius_checked.entry(i).or_insert_with(|| radius_ok(i))
            {
                *why.entry("nodesInRadius (opaque export)").or_default() += 1;
                continue;
            }
        }
        if matches!(m.kind, MismatchKind::Value)
            && m.pob.as_ref().is_some_and(is_lua_fn)
            && m.ours.as_ref().is_some_and(is_jewel_func_spec)
        {
            *why.entry("jewel func (Value::JewelFunc)").or_default() += 1;
            continue;
        }
        if matches!(m.kind, MismatchKind::ModMissing | MismatchKind::ModExtra) {
            // the pair of one JewelFunc mod whose value differs only in the func's representation
            let other = if matches!(m.kind, MismatchKind::ModMissing) {
                MismatchKind::ModExtra
            } else {
                MismatchKind::ModMissing
            };
            let mine = m.pob.as_ref().or(m.ours.as_ref()).map(norm_jewel_funcs);
            let paired = mine.is_some_and(|x| {
                pairs
                    .iter()
                    .any(|(p, k, v)| p == &m.path && *k == other && *v == x)
            });
            if paired {
                *why.entry("jewel func (Value::JewelFunc)").or_default() += 1;
                continue;
            }
        }
        if matches!(m.kind, MismatchKind::Value) && m.pob.as_ref().is_some_and(is_foreign) {
            *why.entry("foreign env object (not this env; PoB's $foreign/$env)")
                .or_default() += 1;
            continue;
        }
        if matches!(m.kind, MismatchKind::Value)
            && m.ours
                .as_ref()
                .and_then(ref_path)
                .is_some_and(|p| p.starts_with("globalCache."))
            && m.pob.as_ref().is_some_and(|v| ref_path(v).is_none())
        {
            // ours reached the object first through another env's cached skill (globalCache,
            // which PoB dumps as `$foreign`), so it exports a ref where PoB writes it inline
            *why.entry("foreign env object (first reached through globalCache)")
                .or_default() += 1;
            continue;
        }
        let gc_ref = |v: &Option<J>| {
            v.as_ref()
                .and_then(ref_path)
                .is_some_and(|p| p.starts_with("globalCache."))
        };
        if matches!(m.kind, MismatchKind::Value)
            && m.path.starts_with("globalCache.")
            && gc_ref(&m.pob)
            && gc_ref(&m.ours)
        {
            // inside the global cache (other envs' skills), both sides refer to an object by its
            // first export path; PoB's `cachedData.MAIN` entries are `$foreign`, so its first
            // path for a shared object is a later mode's entry
            *why.entry("foreign env object (globalCache ref path)")
                .or_default() += 1;
            continue;
        }
        let pob_node = m
            .pob
            .as_ref()
            .and_then(|v| v.get("$node"))
            .and_then(J::as_f64);
        let ours_tree_node = m
            .ours
            .as_ref()
            .and_then(|v| v.get("$treeNode"))
            .and_then(J::as_f64);
        if let (true, Some(id), Some(oid)) = (
            matches!(m.kind, MismatchKind::Value),
            pob_node,
            ours_tree_node,
        ) {
            let id = id as i64;
            let spec_node = env.spec.get().nodes.with(|n| n.get(id));
            let same = oid as i64 == id
                && if m.path == format!("env.allocNodes[{id}]") {
                    // the allocated node is the spec's own node object
                    let a = env.allocNodes.get().with(|n| n.get(id));
                    matches!((a, &spec_node), (Some(a), Some(s)) if Rc::ptr_eq(&a, s))
                } else {
                    // elsewhere (explodeSources, a gem's explodeSource): the spec's node of
                    // that id is one the exporter tags `$treeNode`
                    spec_node.as_ref().is_some_and(|s| s.tree.is_none())
                };
            if same {
                *why.entry("node tag (spec node without a tree overlay)")
                    .or_default() += 1;
                continue;
            }
        }
        // CCR-0010 / calc doc §9.5: the gem resolved to its `AltX`/`AltY` variant in one of the
        // two PoB processes (the oracle's, the data dump's); `nameSpec` is that gem's name
        if matches!(m.kind, MismatchKind::Value) {
            let alt_eq = |a: Option<&J>, b: Option<&J>| match (a, b) {
                (Some(a), Some(b)) => {
                    let n = |v: &J| v.to_string().replace("AltX", "").replace("AltY", "");
                    a != b && n(a) == n(b)
                }
                _ => false,
            };
            let variant = alt_eq(m.pob.as_ref(), m.ours.as_ref())
                || m.path.strip_suffix(".nameSpec").is_some_and(|pre| {
                    let id = format!("{pre}.gemId");
                    alt_eq(lookup(pob, &id), lookup(ours, &id))
                });
            if variant {
                *why.entry("gem variant (CCR-0010)").or_default() += 1;
                continue;
            }
        }
        if m.path.ends_with(".corrupted")
            && matches!(m.kind, MismatchKind::Extra)
            && m.ours.as_ref() == Some(&J::Bool(false))
        {
            *why.entry("nil-default (item field)").or_default() += 1;
            continue;
        }
        // CCR-0113: a copied skill keeps its foreign actor alive in `extra["actorKeep"]`
        // (Rust-only); that actor's `enemy` is an `ActorWeak` whose env (a finished pass's, held
        // by nothing but PoB's global cache) is gone, so it exports nothing
        let actor_keep = m.path.starts_with("globalCache.")
            && ((m.path.ends_with(".actorKeep") && matches!(m.kind, MismatchKind::Extra))
                || (m.path.ends_with(".actor.enemy") && matches!(m.kind, MismatchKind::Missing)));
        if actor_keep {
            *why.entry("port plumbing (CCR-0113 actorKeep, weak enemy)")
                .or_default() += 1;
            continue;
        }
        let plumbing_ref = m
            .ours
            .as_ref()
            .and_then(ref_path)
            .is_some_and(|p| p.starts_with("env.override[\"$extraJewelFuncsActor\"]"));
        if (m.path == "env.override[\"$extraJewelFuncsActor\"]"
            && matches!(m.kind, MismatchKind::Extra))
            || (m.path == "env.override.extraJewelFuncs.actor"
                && matches!(m.kind, MismatchKind::Value)
                && plumbing_ref)
        {
            *why.entry("port plumbing").or_default() += 1;
            continue;
        }
        rest.push(m);
    }
    (rest, why)
}

/// L5: `calcs.initEnv(build, mode)` from the build XML (BM-A's loader, then CS1 → CS2 → CS3) against
/// the `initEnv:end` dump of the first loaded env of that mode without a `specEnv` (MAIN: the main
/// env). Only the env is compared (the model and the locals are L4's). A run that reaches an
/// `unported!` stub (BM-A's `LoadDB`, CS1, CS2 while they are not merged) is reported as
/// stub-blocked with its sites; the gate is that no run raises outside a stub and that every
/// stub-free run matches.
#[test]
fn cs3_l5_init_env() {
    use pob_calc::env::Mode;
    let _serial = serial();
    if pob_testpaths::require(pob_testpaths::dump_dir(), "data/dump").is_none() {
        return;
    }
    assert!(testkit::init_default_data(), "data/dump does not load");
    // Timeless jewel LUTs load from the PoB checkout (as CS2 / BM-A / CA do); the default
    // `StdJewelFs::new("")` probes `./` and `./src` of the test's working directory (the crate),
    // where they are not ("Error occurred loading Glorious Vanity data", FS1).
    let Some(src) = pob_testpaths::require(pob_testpaths::pob_src(), "PoB checkout (src)") else {
        return;
    };
    let src = std::fs::canonicalize(&src).unwrap_or(src);
    pob_gamedata::timeless_lut::set_jewel_fs(std::sync::Arc::new(
        pob_gamedata::timeless_lut::StdJewelFs {
            script_path: src.to_string_lossy().into_owned(),
            writable: false,
        },
    ));
    pob_gamedata::timeless_lut::set_con_printf(|_| {});
    let Some(builds) = builds() else { return };
    let (mut pass, mut fail, mut blocked, mut errors, mut no_target) = (0, 0, 0, 0, 0);
    let mut sites: BTreeMap<String, usize> = BTreeMap::new();
    let mut explained: BTreeMap<&'static str, usize> = BTreeMap::new();
    let mut lines = Vec::new();
    for xml in &builds {
        pob_calc::modules::calc_setup::init_env_skills::reset_grantedEffect_fromItem();
        let stem = stem(xml);
        let dir = match ensure_dumps(xml, &stem) {
            Ok(d) => d,
            Err(e) => {
                lines.push(format!("missing {stem}: {e}"));
                continue;
            }
        };
        let main_env: Option<String> = std::fs::read_to_string(dir.join("hits.json"))
            .ok()
            .and_then(|s| serde_json::from_str::<J>(&s).ok())
            .and_then(|j| j.get("mainEnv").and_then(J::as_str).map(str::to_owned));
        let outs = hit_files(&dir.join("state"), "initEnv:end");
        for (mode, name) in [
            (Mode::MAIN, "MAIN"),
            (Mode::CALCS, "CALCS"),
            (Mode::CALCULATOR, "CALCULATOR"),
        ] {
            let mut target: Option<StateDump> = None;
            for p in outs.values() {
                let Ok(d) = StateDump::load(p) else { continue };
                let h = hit_info(&d);
                // top-level calls only (FS1): an initEnv inside `calcs.perform` (CalcMirages'
                // CALCULATOR envs, stack `perform:in, mirages:in, initEnv:in`) is not one of
                // CalcsTab:BuildOutput's own, which `calcs_tab_prefix` reproduces (initEnv's own
                // restart, CalcSetup.lua 1433 / 1923, nests `initEnv:in` only and stays)
                let ok = h.mode == name
                    && !h.spec_env
                    && h.loaded
                    && d.stack.iter().all(|s| s.starts_with("initEnv:in#"))
                    && (mode != Mode::MAIN || main_env.as_deref() == Some(h.env_id.as_str()));
                if ok {
                    target = Some(d);
                    break;
                }
            }
            let Some(dout) = target else {
                no_target += 1;
                lines.push(format!("no-dump {stem} {name}"));
                continue;
            };
            pob_calc::stub::stub_reset();
            // BM-A's loader (`Build:Init`'s model half and `LoadDB`, the tree included, without
            // the first BuildOutput). FS1: this used `testkit::load_build` (`Build::new` +
            // `LoadDB` only, the pre-BM-A stand-in), whose spec has no tree, so CalcSetup.lua:593
            // indexed a nil `classStats` in every run ("attempt to index a nil value", 171/171).
            let text = std::fs::read_to_string(xml).unwrap_or_default();
            let run = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                pob_calc::modules::common::wipeGlobalCache();
                let build =
                    pob_calc::model::build_xml::loadBuildModelFromXML(&text, pob_gamedata::data())
                        .unwrap_or_else(|e| panic!("load: {e}"));
                build.outputRevision.set(1.0);
                calcs_tab_prefix(&build, mode);
                pob_calc::modules::calc_setup::initEnv(&build, mode, None, None).0
            }));
            let stubs: Vec<String> = pob_calc::stub::stub_report()
                .iter()
                .map(|(site, n)| format!("{site} x{n}"))
                .collect();
            if !stubs.is_empty() {
                blocked += 1;
                for s in &stubs {
                    *sites
                        .entry(s.split(" x").next().unwrap_or("").to_owned())
                        .or_default() += 1;
                }
                lines.push(format!("BLOCKED {stem} {name}: {}", stubs.join(", ")));
                continue;
            }
            let env = match run {
                Ok(e) => e,
                Err(p) => {
                    errors += 1;
                    lines.push(format!("ERROR   {stem} {name}: {}", panic_msg(&*p)));
                    continue;
                }
            };
            let mut d = dout.clone();
            if let Some(J::Object(l)) = d.root.get_mut("locals") {
                l.clear();
            }
            let (mm, pob_tree, ours_tree, reordered) = diff_with_trees(&d, Some(&env), &[], &stem);
            if let Some(dir) = std::env::var_os("CS3_TREES") {
                // debug knob: the two resolved trees of each L5 run
                let dir = PathBuf::from(dir);
                let _ = std::fs::create_dir_all(&dir);
                let _ = std::fs::write(
                    dir.join(format!("{stem}.{name}.pob.json")),
                    pob_tree.to_string(),
                );
                let _ = std::fs::write(
                    dir.join(format!("{stem}.{name}.ours.json")),
                    ours_tree.to_string(),
                );
            }
            let (rest, why) = classify(mm, &pob_tree, &ours_tree, reordered);
            let (rest, why5) = classify_l5(rest, &pob_tree, &ours_tree, &env);
            for (k, n) in why.into_iter().chain(why5) {
                *explained.entry(k).or_default() += n;
            }
            release(&env);
            if rest.is_empty() {
                pass += 1;
                lines.push(format!("ok      {stem} {name}"));
            } else {
                fail += 1;
                lines.push(format!("FAIL    {stem} {name}: {} mismatches", rest.len()));
                let shown = if std::env::var_os("CS3_VERBOSE").is_some() {
                    rest.len()
                } else {
                    8
                };
                for m in rest.iter().take(shown) {
                    lines.push(format!("          {}", fmt_mismatch(m)));
                }
            }
        }
    }
    for l in &lines {
        eprintln!("{l}");
    }
    eprintln!(
        "CS3 L5 initEnv:out: pass {pass} / fail {fail} / stub-blocked {blocked} / errors {errors} / no dump {no_target}"
    );
    for (s, n) in &sites {
        eprintln!("  stub {s}: blocked {n} runs");
    }
    for (k, n) in &explained {
        eprintln!("  explained {k}: {n}");
    }
    assert_eq!(
        errors, 0,
        "CS3 L5:{errors} runs raised outside a stub (see stderr)"
    );
    assert_eq!(
        fail, 0,
        "CS3 L5: {fail} stub-free runs mismatch (see stderr)"
    );
}
