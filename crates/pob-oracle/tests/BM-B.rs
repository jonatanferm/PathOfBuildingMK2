//! Unit BM-B acceptance (calc doc §11.1, row BM-B): the skills, config and
//! party tab models against PoB under LuaJIT (the oracle, `jit.off()`).
//!
//! * `corpus_tabs_match_oracle` — for every `.parity/builds/*.xml` build:
//!   - **skills**: `skillsTab` (skill sets, `socketGroupList`, every gem field
//!     ProcessSocketGroup fills, `matchesSocket` from UpdateSocketGroups) after
//!     `Load` + `UpdateSocketGroups` equals PoB's at Build.lua:691-694 (captured
//!     by hooking the first `calcsTab:BuildOutput`, before any calc touches
//!     the groups). Items are injected from the oracle (slot → selected item →
//!     sockets) instead of being loaded (BM-A/IT-*).
//!   - **config**: config sets (input, placeholder, custom mod blocks), the
//!     active set binding, `enemyLevel` (UpdateLevel) and `defaultState`.
//!   - **party**: the `<Party>` section Load (buff lists, party member stats,
//!     enemy mods/conditions, labels).
//!   - **Save**: the Rust `Save` of each tab equals PoB's `Save` node for node
//!     (config children compared as a multiset: PoB writes `pairs` order).
//!   - **round trip**: Rust `Save` → Rust `Load` (fresh build) reproduces the
//!     same model dump.
//!   - **ParseBuffs / exportBuffs**: PoB exports the build's party buffs
//!     (`enableExportBuffs`, CalcPerform → `setBuffExports` → `exportBuffs`);
//!     the Rust `ParseBuffs` of those strings equals PoB's (mod lists compared as
//!     `modLib.formatSourceMod` lines), and the Rust `exportBuffs` of the
//!     injected `buffExports` gives the same text (blocks compared as a
//!     multiset: PoB iterates with `pairs`).
//! * `synthetic_sections_match_oracle` — `corpus/BM-B/synthetic_*.xml`
//!   (several skill/config sets, every gem lookup path, legacy formats, the
//!   config load quirks, empty sections) loaded into PoB's tabs and into Rust.
//!
//! Values that depend on unported units are reported as *pending* when the
//! Rust phase hit the corresponding `unported!` stubs (CT: `getGemStatRequirement`,
//! `validateGemLevel`, `modLib.*`; MS-A: ModStore/ModDB/ModList storage); every
//! other difference fails the test. With CT and MS-A merged nothing is pending.
//!
//! A missing input (data/dump, the PoB checkout, .parity/builds) fails
//! (`pob_testpaths::require`; `POB_ALLOW_SKIP=1` skips instead).
//! `BMB_BUILDS=<substring>` restricts the corpus run.
#![allow(non_snake_case, clippy::pedantic, clippy::type_complexity)]

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use pob_calc::model::config_tab::defaultState;
use pob_calc::model::party_tab::{buff_modList, BuffList};
use pob_calc::model::skills_tab::{
    skillSet_socketGroupList, socketGroup_includeInFullDPS, socketGroup_label,
};
use pob_calc::model::{Build, BuildRef, XmlChild, XmlNode};
use pob_calc::prelude::*;
use pob_oracle::headless::Engine;
use pob_oracle::jit::JitPob;
use pob_oracle::oracle::{find_parity_dir, find_pob_src, find_up, ORACLE_LUA, QUIET_PRELUDE};
use pob_oracle::runner::OracleLock;

const BMB_LUA: &str = include_str!("../corpus/BM-B/bmb.lua");

/// The stub counters are process-global: run the tests of this file one at a time.
static SERIAL: Mutex<()> = Mutex::new(());

fn dump_dir() -> Option<PathBuf> {
    if let Some(p) = std::env::var_os("POB_DUMP_DIR").map(PathBuf::from) {
        return p.join("data.bin").is_file().then_some(p);
    }
    find_up("data/dump/data.bin").map(|p| p.parent().unwrap().to_path_buf())
}

fn init_data() -> Option<&'static Data> {
    let dir = dump_dir()?;
    pob_gamedata::init_from_dir(&dir).unwrap_or_else(|e| panic!("data.bin: {e}"));
    Some(pob_gamedata::data())
}

// ---- line dumps ---------------------------------------------------------------------------

/// `path -> encoded value` (the encoding of bmb.lua's `enc`).
type Lines = BTreeMap<String, String>;

fn esc(s: &str) -> String {
    let mut o: Vec<u8> = Vec::with_capacity(s.len());
    for b in s.bytes() {
        if b < 32 || b == 127 || b == b'\\' {
            o.extend_from_slice(format!("\\{b:03}").as_bytes());
        } else {
            o.push(b);
        }
    }
    String::from_utf8_lossy(&o).into_owned()
}

fn enc(v: &Val) -> Option<String> {
    match v {
        Val::Nil => None,
        Val::Bool(b) => Some(format!("b:{b}")),
        Val::Num(n) => Some(format!("n:{}", lfmt!("%.17g", *n))),
        Val::Str(s) => Some(format!("s:{}", esc(s))),
        other => Some(format!("t:{}", other.lua_type())),
    }
}

#[derive(Default)]
struct Dump {
    lines: Lines,
}

impl Dump {
    fn emit(&mut self, path: impl Into<String>, v: impl Into<Val>) {
        if let Some(e) = enc(&v.into()) {
            self.lines.insert(path.into(), e);
        }
    }
}

fn name_val(n: Option<Name>) -> Val {
    n.map_or(Val::Nil, |n| Val::from(n.as_str()))
}
fn nan_nil(v: f64) -> Val {
    if v.is_nan() {
        Val::Nil
    } else {
        Val::Num(v)
    }
}
fn key_str(k: &Key) -> String {
    match k {
        Key::S(s) => s.to_string(),
        Key::I(i) => tostring_num(*i as f64),
        Key::B(b) => b.to_string(),
    }
}

fn dump_groups(d: &mut Dump, prefix: &str, list: &Shared<Vec<SocketGroupRef>>) {
    let groups = list.snapshot();
    d.emit(format!("{prefix}.n"), groups.len() as f64);
    for (i, g) in groups.iter().enumerate() {
        let p = format!("{prefix}[{}]", i + 1);
        d.emit(
            format!("{p}.label"),
            socketGroup_label(g).map_or(Val::Nil, Val::Str),
        );
        d.emit(format!("{p}.enabled"), g.enabled.get());
        d.emit(
            format!("{p}.includeInFullDPS"),
            socketGroup_includeInFullDPS(g),
        );
        d.emit(format!("{p}.groupCount"), g.groupCount.get());
        d.emit(format!("{p}.slot"), name_val(g.slot.get()));
        d.emit(
            format!("{p}.source"),
            g.source.get().map_or(Val::Nil, Val::Str),
        );
        d.emit(format!("{p}.mainActiveSkill"), g.mainActiveSkill.get());
        d.emit(
            format!("{p}.mainActiveSkillCalcs"),
            g.mainActiveSkillCalcs.get(),
        );
        d.emit(
            format!("{p}.imbuedSupport"),
            g.extra.get(n!("imbuedSupport")),
        );
        let gems = g.gemList.get().snapshot();
        d.emit(format!("{p}.gems.n"), gems.len() as f64);
        for (j, gem) in gems.iter().enumerate() {
            let q = format!("{p}.gems[{}]", j + 1);
            d.emit(format!("{q}.nameSpec"), Val::Str(gem.nameSpec.get()));
            d.emit(format!("{q}.skillId"), name_val(gem.skillId.get()));
            d.emit(format!("{q}.gemId"), name_val(gem.gemId.get()));
            d.emit(format!("{q}.level"), nan_nil(gem.level.get()));
            d.emit(format!("{q}.quality"), nan_nil(gem.quality.get()));
            d.emit(format!("{q}.enabled"), gem.enabled.get());
            d.emit(format!("{q}.enableGlobal1"), gem.enableGlobal1.get());
            d.emit(format!("{q}.enableGlobal2"), gem.enableGlobal2.get());
            d.emit(format!("{q}.count"), gem.count.get());
            d.emit(format!("{q}.skillPart"), gem.skillPart.get());
            d.emit(format!("{q}.skillPartCalcs"), gem.skillPartCalcs.get());
            d.emit(format!("{q}.skillStageCount"), gem.skillStageCount.get());
            d.emit(
                format!("{q}.skillStageCountCalcs"),
                gem.skillStageCountCalcs.get(),
            );
            d.emit(format!("{q}.skillMineCount"), gem.skillMineCount.get());
            d.emit(
                format!("{q}.skillMineCountCalcs"),
                gem.skillMineCountCalcs.get(),
            );
            d.emit(format!("{q}.skillMinion"), name_val(gem.skillMinion.get()));
            d.emit(
                format!("{q}.skillMinionCalcs"),
                name_val(gem.skillMinionCalcs.get()),
            );
            d.emit(
                format!("{q}.skillMinionItemSet"),
                gem.skillMinionItemSet.get(),
            );
            d.emit(
                format!("{q}.skillMinionItemSetCalcs"),
                gem.skillMinionItemSetCalcs.get(),
            );
            d.emit(format!("{q}.skillMinionSkill"), gem.skillMinionSkill.get());
            d.emit(
                format!("{q}.skillMinionSkillCalcs"),
                gem.skillMinionSkillCalcs.get(),
            );
            for f in [
                "color",
                "errMsg",
                "reqStr",
                "reqDex",
                "reqInt",
                "matchesSocket",
                "naturalMaxLevel",
                "new",
            ] {
                d.emit(format!("{q}.{f}"), gem.extra.get(Name::from(f)));
            }
            d.emit(format!("{q}.reqLevel"), gem.reqLevel.get());
            d.emit(
                format!("{q}.triggered"),
                if gem.triggered.get() {
                    Val::Bool(true)
                } else {
                    Val::Nil
                },
            );
            d.emit(
                format!("{q}.gemData"),
                name_val(gem.gemData.get().map(|g| g.id)),
            );
            d.emit(
                format!("{q}.grantedEffect"),
                name_val(gem.grantedEffect.get().map(|g| g.id)),
            );
        }
    }
}

fn dump_skills(d: &mut Dump, st: &SkillsTab) {
    d.emit("skills.activeSkillSetId", st.activeSkillSetId.get());
    d.emit("skills.defaultGemLevel", name_val(st.defaultGemLevel.get()));
    d.emit("skills.defaultGemQuality", st.defaultGemQuality.get());
    let ui = st.ui_state();
    for k in [
        "sortGemsByDPS",
        "sortGemsByDPSField",
        "showSupportGemTypes",
        "showLegacyGems",
    ] {
        d.emit(format!("skills.{k}"), ui.get(Name::from(k)));
    }
    for (i, id) in st.skillSetOrderList.snapshot().into_iter().enumerate() {
        d.emit(format!("skills.order[{}]", i + 1), f64::from(id));
        let set = st
            .skillSets
            .with(|m| m.get(&id).cloned())
            .expect("skill set");
        d.emit(format!("skills.sets[{id}].title"), set.get(n!("title")));
        dump_groups(
            d,
            &format!("skills.sets[{id}].groups"),
            &skillSet_socketGroupList(&set),
        );
    }
    let activeId = st.activeSkillSetId.get();
    let active = (activeId >= 0.0 && activeId.fract() == 0.0)
        .then(|| st.skillSets.with(|m| m.get(&(activeId as u32)).cloned()))
        .flatten();
    d.emit(
        "skills.activeIsSet",
        active.is_some_and(|a| skillSet_socketGroupList(&a).ptr_eq(&st.socketGroupList.get())),
    );
    dump_groups(d, "skills.groups", &st.socketGroupList.get());
    if let Some(dg) = st.displayGroup.get() {
        for (i, g) in st.socketGroupList.get().snapshot().iter().enumerate() {
            if Rc::ptr_eq(g, &dg) {
                d.emit("skills.displayGroup", (i + 1) as f64);
            }
        }
    }
    for (k, v) in st.imbuedSupportBySlot.pairs() {
        let ge = v
            .obj::<GrantedEffectRef>()
            .expect("imbued support granted effect");
        d.emit(format!("skills.imbued.{}", key_str(&k)), ge.id.as_str());
    }
}

fn dump_config(d: &mut Dump, ct: &ConfigTab) {
    d.emit("cfg.activeConfigSetId", ct.activeConfigSetId.get());
    for (i, id) in ct.configSetOrderList.snapshot().into_iter().enumerate() {
        d.emit(format!("cfg.order[{}]", i + 1), f64::from(id));
    }
    let sets: Vec<(u32, Table)> = ct
        .configSets
        .with(|m| m.iter().map(|(k, v)| (*k, v.clone())).collect());
    for (id, set) in &sets {
        let p = format!("cfg.sets[{id}]");
        d.emit(format!("{p}.id"), set.get(n!("id")));
        d.emit(format!("{p}.title"), set.get(n!("title")));
        for (k, v) in set.table(n!("input")).unwrap().pairs() {
            d.emit(format!("{p}.input.{}", key_str(&k)), v);
        }
        for (k, v) in set.table(n!("placeholder")).unwrap().pairs() {
            d.emit(format!("{p}.placeholder.{}", key_str(&k)), v);
        }
        if let Some(cml) = set.table(n!("customModsList")) {
            d.emit(format!("{p}.cml.n"), cml.len() as f64);
            for (i, b) in cml.ipairs() {
                for f in ["title", "enabled", "text"] {
                    d.emit(format!("{p}.cml[{i}].{f}"), b.get(Name::from(f)));
                }
            }
        }
    }
    let active = ct.configSet(ct.activeConfigSetId.get());
    d.emit(
        "cfg.inputIsActive",
        active
            .as_ref()
            .is_some_and(|a| a.table(n!("input")).unwrap().ptr_eq(&ct.input)),
    );
    d.emit(
        "cfg.placeholderIsActive",
        active
            .as_ref()
            .is_some_and(|a| a.table(n!("placeholder")).unwrap().ptr_eq(&ct.placeholder)),
    );
    d.emit("cfg.enemyLevel", ct.enemyLevel.get());
}

fn dump_entry(d: &mut Dump, p: &str, e: &Val) {
    d.emit(format!("{p}.effectMult"), e.get(n!("effectMult")));
    d.emit(format!("{p}.isMark"), e.get(n!("isMark")));
    let mods = buff_modList(e);
    for (i, m) in mods.iter().enumerate() {
        d.emit(
            format!("{p}.mod[{}]", i + 1),
            pob_gamedata::mod_tools::formatSourceMod(m).as_str(),
        );
    }
    d.emit(format!("{p}.mods.n"), mods.len() as f64);
}

fn dump_buff_list(d: &mut Dump, prefix: &str, list: &Table) {
    for (ty, entries) in list.pairs() {
        let tys = key_str(&ty);
        if tys == "limit" {
            d.emit(format!("{prefix}.limit"), entries);
        } else if let Some(t) = entries.table() {
            for (name, e) in t.pairs() {
                let ns = key_str(&name);
                if ns == "Vaal" && e.get(n!("modList")).is_nil() {
                    for (vn, ve) in e.table().unwrap().pairs() {
                        dump_entry(d, &format!("{prefix}.{tys}.Vaal.{}", key_str(&vn)), &ve);
                    }
                } else {
                    dump_entry(d, &format!("{prefix}.{tys}.{ns}"), &e);
                }
            }
        }
    }
}

fn dump_store(d: &mut Dump, prefix: &str, store: &ModStoreRef) {
    match store.kind {
        StoreKind::ModDB => {
            let names: BTreeSet<String> = store
                .all_mods()
                .iter()
                .map(|m| m.name.to_string())
                .collect();
            for name in names {
                for (i, m) in store
                    .mods_named(Name::from(name.as_str()))
                    .iter()
                    .enumerate()
                {
                    d.emit(
                        format!("{prefix}.{name}[{}]", i + 1),
                        pob_gamedata::mod_tools::formatSourceMod(m).as_str(),
                    );
                }
            }
        }
        StoreKind::ModList => {
            for (i, m) in store.ipairs().iter().enumerate() {
                d.emit(
                    format!("{prefix}[{}]", i + 1),
                    pob_gamedata::mod_tools::formatSourceMod(m).as_str(),
                );
            }
        }
    }
}

fn dump_output(d: &mut Dump, prefix: &str, output: &Table) {
    for (k, v) in output.pairs() {
        if let Some(t) = v.table() {
            for (k2, v2) in t.pairs() {
                d.emit(format!("{prefix}.{}.{}", key_str(&k), key_str(&k2)), v2);
            }
        } else {
            d.emit(format!("{prefix}.{}", key_str(&k)), v);
        }
    }
}

const LABELS: [&str; 6] = [
    "simpleAuras",
    "simpleCurses",
    "simpleWarcries",
    "simpleLinks",
    "simpleEnemyMods",
    "simpleEnemyCond",
];
const BUFS: [&str; 7] = [
    "editPartyMemberStats",
    "editAuras",
    "editCurses",
    "editWarcries",
    "editLinks",
    "enemyCond",
    "enemyMods",
];

fn dump_party(d: &mut Dump, pt: &PartyTab) {
    for k in ["Aura", "Curse", "Warcry", "Link"] {
        dump_buff_list(d, &format!("party.{k}"), &pt.actorList(Name::from(k)));
    }
    dump_store(d, "party.modDB", &pt.actor.modDB.get());
    dump_output(d, "party.output", &pt.actor.output.get());
    dump_store(d, "party.enemyModList", &pt.enemyModList.get());
    for c in LABELS {
        d.emit(format!("party.label.{c}"), pt.control(c).get(n!("label")));
    }
    for c in BUFS {
        d.emit(format!("party.buf.{c}"), pt.control(c).get(n!("buf")));
    }
}

// ---- oracle output parsing ----------------------------------------------------------------

struct LuaDump {
    lines: Lines,
    json: BTreeMap<String, serde_json::Value>,
}

fn parse_lua(text: &str) -> LuaDump {
    assert!(
        !text.starts_with("error\t"),
        "oracle capture failed: {text}"
    );
    let mut lines = Lines::new();
    let mut json = BTreeMap::new();
    for line in text.split('\n').filter(|l| !l.is_empty()) {
        let (k, v) = line
            .split_once('\t')
            .unwrap_or_else(|| panic!("bad oracle line {line:?}"));
        if let Some(j) = v.strip_prefix("j:") {
            json.insert(
                k.to_owned(),
                serde_json::from_str(j).unwrap_or_else(|e| panic!("{k}: {e}")),
            );
        } else {
            lines.insert(k.to_owned(), v.to_owned());
        }
    }
    LuaDump { lines, json }
}

/// A Lua xml node as bmb.lua's `xmlJson` writes it (`{"elem","attrib","children"}`).
fn xml_from_json(v: &serde_json::Value) -> XmlNode {
    let o = v.as_object().expect("xml node object");
    let mut attrib = indexmap::IndexMap::new();
    if let Some(a) = o.get("attrib").and_then(|a| a.as_object()) {
        for (k, v) in a {
            if let Some(s) = v.as_str() {
                attrib.insert(LStr::from(k.as_str()), LStr::from(s));
            }
        }
    }
    let children = o
        .get("children")
        .and_then(|c| c.as_array())
        .map(|a| {
            a.iter()
                .map(|v| match v {
                    serde_json::Value::String(s) => XmlChild::Text(LStr::from(s.as_str())),
                    other => XmlChild::Node(xml_from_json(other)),
                })
                .collect()
        })
        .unwrap_or_default();
    XmlNode {
        elem: LStr::from(o.get("elem").and_then(|e| e.as_str()).unwrap_or("")),
        attrib,
        children,
    }
}

/// Canonical text of a node; `unordered` sorts the children of every node below the top level.
fn canon(n: &XmlNode, unordered_below: bool, depth: usize) -> String {
    let mut a: Vec<String> = n.attrib.iter().map(|(k, v)| format!("{k}={v:?}")).collect();
    a.sort();
    let mut c: Vec<String> = n
        .children
        .iter()
        .map(|c| match c {
            XmlChild::Text(t) => format!("{t:?}"),
            XmlChild::Node(x) => canon(x, unordered_below, depth + 1),
        })
        .collect();
    if unordered_below && depth >= 1 {
        c.sort();
    }
    format!("<{} {}>[{}]", n.elem, a.join(" "), c.join(","))
}

// ---- comparison ---------------------------------------------------------------------------

#[derive(Default)]
struct Report {
    compared: usize,
    pending: usize,
    /// PoB's own nondeterminism (LUA-ORDER, calc doc §3.4/§9.5): a value PoB picks by `pairs`.
    tolerated: usize,
    mismatches: Vec<String>,
    pending_examples: Vec<String>,
    notes: BTreeSet<String>,
}

fn num_of(s: &str) -> Option<f64> {
    s.strip_prefix("n:").and_then(|n| match n {
        "nan" | "-nan" => Some(f64::NAN),
        "inf" => Some(f64::INFINITY),
        "-inf" => Some(f64::NEG_INFINITY),
        _ => n.parse().ok(),
    })
}

fn same(a: Option<&String>, b: Option<&String>) -> bool {
    match (a, b) {
        (Some(x), Some(y)) if x == y => true,
        (Some(x), Some(y)) => match (num_of(x), num_of(y)) {
            (Some(p), Some(q)) => p == q || (p.is_nan() && q.is_nan()),
            _ => false,
        },
        (None, None) => true,
        _ => false,
    }
}

fn compare(
    what: &str,
    lua: &Lines,
    rust: &Lines,
    prefixes: &[&str],
    pending: &dyn Fn(&str) -> bool,
    r: &mut Report,
) {
    compare_t(what, lua, rust, prefixes, pending, &|_| false, r);
}

fn compare_t(
    what: &str,
    lua: &Lines,
    rust: &Lines,
    prefixes: &[&str],
    pending: &dyn Fn(&str) -> bool,
    tolerate: &dyn Fn(&str) -> bool,
    r: &mut Report,
) {
    let keys: BTreeSet<&String> = lua
        .keys()
        .chain(rust.keys())
        .filter(|k| prefixes.iter().any(|p| k.starts_with(p)))
        .collect();
    for k in keys {
        let (a, b) = (lua.get(k), rust.get(k));
        r.compared += 1;
        if same(a, b) {
            continue;
        }
        let msg = format!("{what}: {k}: lua={a:?} rust={b:?}");
        if tolerate(k) {
            r.tolerated += 1;
        } else if pending(k) {
            r.pending += 1;
            if r.pending_examples.len() < 8 {
                r.pending_examples.push(msg);
            }
        } else {
            r.mismatches.push(msg);
        }
    }
}

/// LUA-ORDER: a party label PoB builds with `pairs` (ParseBuffs `enemyModList`, PartyTab.lua:748;
/// `otherEffects`, 939): the same lines in another order.
fn lua_order_label(k: &str, lua: &Lines, rust: &Lines) -> bool {
    if !k.starts_with("party.label.") {
        return false;
    }
    let lines = |v: Option<&String>| -> Option<Vec<String>> {
        let mut l: Vec<String> = v?.split("\\010").map(str::to_owned).collect();
        l.sort();
        Some(l)
    };
    match (lines(lua.get(k)), lines(rust.get(k))) {
        (Some(a), Some(b)) => a == b,
        _ => false,
    }
}

/// `<prefix>.gems[n]` of a gem field path.
fn gem_prefix(k: &str) -> Option<&str> {
    let i = k.rfind(".gems[")?;
    let j = k[i..].find(']')? + i;
    Some(&k[..=j])
}

/// Two gem records PoB cannot tell apart when loading the old format: the variants of one
/// `gameId` that share the `grantedEffectId` (SkillsTab.lua:474-479 picks the first in `pairs`
/// order, which LuaJIT randomises per process: Vaal X / Vaal X AltX).
fn ambiguous_variants(data: &'static Data, a: &str, b: &str) -> bool {
    let (Some(ga), Some(gb)) = (data.gems.get(&Name::from(a)), data.gems.get(&Name::from(b)))
    else {
        return false;
    };
    ga.gameId == gb.gameId && ga.grantedEffectId == gb.grantedEffectId
}

/// LUA-ORDER: a gem whose record PoB picked among ambiguous variants (every field of that gem
/// follows from the record).
fn lua_order_gem(data: &'static Data, k: &str, lua: &Lines, rust: &Lines) -> bool {
    let Some(p) = gem_prefix(k) else { return false };
    let key = format!("{p}.gemData");
    match (lua.get(&key), rust.get(&key)) {
        (Some(a), Some(b)) if a != b => match (a.strip_prefix("s:"), b.strip_prefix("s:")) {
            (Some(a), Some(b)) => ambiguous_variants(data, a, b),
            _ => false,
        },
        _ => false,
    }
}

/// Mask `variantId`/`nameSpec` of saved old-format gems whose record is one of several
/// ambiguous variants (LUA-ORDER); returns how many were masked.
fn mask_ambiguous(data: &'static Data, n: &mut XmlNode) -> usize {
    let mut masked = 0;
    if &*n.elem == "Gem" {
        let gameId = n.attrib.get("gemId").map(|s| s.to_string());
        let skillId = n.attrib.get("skillId").map(|s| s.to_string());
        if let (Some(gameId), Some(skillId)) = (gameId, skillId) {
            let count = data
                .gemsByGameId
                .0
                .get(&Name::from(gameId.as_str()))
                .map_or(0, |v| {
                    v.values()
                        .filter(|g| g.grantedEffectId.as_str() == skillId)
                        .count()
                });
            if count > 1 {
                for k in ["variantId", "nameSpec"] {
                    if n.attrib.contains_key(k) {
                        n.attrib.insert(LStr::from(k), LStr::from("<LUA-ORDER>"));
                    }
                }
                masked += 1;
            }
        }
    }
    for c in &mut n.children {
        if let XmlChild::Node(c) = c {
            masked += mask_ambiguous(data, c);
        }
    }
    masked
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

/// Which keys depend on a stub that was hit (CT / MS-A not merged yet).
struct Pending {
    gemReq: bool,
    gemLevel: bool,
    party: bool,
}

impl Pending {
    fn from_hits(h: &BTreeSet<&'static str>) -> Self {
        Pending {
            gemReq: h.contains("Modules/CalcTools.lua:179-189"),
            gemLevel: h.contains("Modules/CalcTools.lua:42-58"),
            party: h.iter().any(|s| {
                s.starts_with("Modules/ModTools.lua")
                    || s.starts_with("Classes/ModStore.lua")
                    || s.starts_with("Classes/ModDB.lua")
                    || s.starts_with("Classes/ModList.lua")
            }),
        }
    }
    fn key(&self, k: &str) -> bool {
        (self.gemReq
            && (k.ends_with(".reqStr") || k.ends_with(".reqDex") || k.ends_with(".reqInt")))
            || (self.gemLevel
                && (k.ends_with(".level")
                    || k.ends_with(".reqLevel")
                    || k.ends_with(".reqStr")
                    || k.ends_with(".reqDex")
                    || k.ends_with(".reqInt")))
            || (self.party
                && (k.starts_with("party.")
                    && !k.starts_with("party.buf.")
                    && !k.starts_with("party.output.")))
            || (self.party && (k.starts_with("str.") || k.starts_with("perr.")))
    }
}

// ---- Rust side ----------------------------------------------------------------------------

/// Items and slots from the oracle's `inj.*` lines (what BM-A/IT-* would load).
fn new_build(data: &'static Data, lua: &Lines) -> BuildRef {
    let build = Build::new(data);
    // The ConfigTab constructor ran in PoB's Build:Init before Load (Build.lua:611): config set
    // 1 (ConfigTab.lua:145-148) and one control per option (ConfigTab.lua:672-681). Load's
    // SetActiveConfigSet runs BuildModList (ConfigTab.lua:1358), whose applies index those
    // controls (ConfigOptions.lua:789 `varControls['enemyRadius']:SetPlaceholder`).
    build.configTab.ctor_model();
    build.configTab.ctor_controls_model(data);
    if let Some(cl) = lua.get("inj.characterLevel").and_then(|v| num_of(v)) {
        build.characterLevel.set(cl);
    }
    let mut items: BTreeMap<u32, (usize, Vec<Option<String>>, bool)> = BTreeMap::new();
    for (k, v) in lua {
        if let Some(rest) = k.strip_prefix("inj.slot.") {
            let t = Table::new();
            t.set(n!("selItemId"), num_of(v).expect("selItemId"));
            build.itemsTab.slots.with_mut(|m| {
                m.insert(Name::from(rest), t);
            });
        } else if let Some(rest) = k.strip_prefix("inj.item.") {
            let (id, field) = rest.split_once('.').expect("inj.item.<id>.<field>");
            let e = items.entry(id.parse().expect("item id")).or_default();
            if field == "n" {
                e.0 = num_of(v).expect("n") as usize;
            } else if field == "cam" {
                e.2 = v == "b:true";
            } else if let Some(i) = field
                .strip_prefix("sock[")
                .and_then(|f| f.strip_suffix(']'))
            {
                let i: usize = i.parse().expect("socket index");
                if e.1.len() < i {
                    e.1.resize(i, None);
                }
                e.1[i - 1] = v.strip_prefix("s:").map(str::to_owned);
            }
        }
    }
    for (id, (n, socks, cam)) in items {
        let item = pob_calc::classes::Item::default();
        let sockets: Vec<Table> = (0..n)
            .map(|i| {
                let t = Table::new();
                if let Some(Some(c)) = socks.get(i) {
                    t.set(n!("color"), c.as_str());
                }
                t
            })
            .collect();
        item.sockets.set(Shared::new(sockets));
        if cam {
            item.extra.set(n!("colourAlwaysMatches"), true);
        }
        build.itemsTab.items.with_mut(|m| {
            m.insert(id, Rc::new(item));
        });
    }
    build
}

fn catch_lua<R>(what: &str, f: impl FnOnce() -> R) -> Result<R, String> {
    match std::panic::catch_unwind(std::panic::AssertUnwindSafe(f)) {
        Ok(r) => Ok(r),
        Err(p) => {
            let msg = if let Some(e) = p.downcast_ref::<LuaError>() {
                e.0.clone()
            } else if let Some(s) = p.downcast_ref::<String>() {
                s.clone()
            } else if let Some(s) = p.downcast_ref::<&str>() {
                (*s).to_owned()
            } else {
                "panic".to_owned()
            };
            Err(format!("{what}: {msg}"))
        }
    }
}

/// Load the sections present in `xml` into `build` (skills + UpdateSocketGroups, config +
/// UpdateLevel as BuildModList would, party) and dump them.
fn rust_load(build: &BuildRef, xml: &BTreeMap<String, XmlNode>) -> Result<Dump, String> {
    let mut d = Dump::default();
    if let Some(n) = xml.get("Skills") {
        catch_lua("skills Load", || {
            build.skillsTab.Load(build, n, "");
            build.skillsTab.UpdateSocketGroups(build);
        })?;
        catch_lua("skills dump", || dump_skills(&mut d, &build.skillsTab))?;
    }
    if let Some(n) = xml.get("Config") {
        catch_lua("config Load", || {
            build.configTab.Load(build, n, "");
            // ConfigTab:BuildModList (CO-C) starts with UpdateLevel (ConfigTab.lua:1138)
            build.configTab.UpdateLevel(build);
        })?;
        catch_lua("config dump", || dump_config(&mut d, &build.configTab))?;
    }
    if let Some(n) = xml.get("Party") {
        catch_lua("party Load", || build.partyTab.Load(build, n, ""))?;
        catch_lua("party dump", || dump_party(&mut d, &build.partyTab))?;
    }
    Ok(d)
}

fn rust_save(build: &BuildRef, elem: &str) -> Result<XmlNode, String> {
    let mut n = XmlNode {
        elem: LStr::from(elem),
        attrib: indexmap::IndexMap::new(),
        children: Vec::new(),
    };
    catch_lua(&format!("{elem} Save"), || match elem {
        "Skills" => build.skillsTab.Save(&mut n),
        "Config" => build.configTab.Save(&mut n),
        _ => build.partyTab.Save(&mut n),
    })?;
    Ok(n)
}

/// Lua value from the oracle's JSON (`buffExports` injection): mods become `Val::Mod`.
fn val_from_json(v: &serde_json::Value) -> Val {
    use serde_json::Value as J;
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
            if o.contains_key("name") && o.contains_key("type") && o.contains_key("tags") {
                return Val::Mod(Arc::new(mod_from_json(v)));
            }
            if o.len() == 1 {
                if let Some(J::String(s)) = o.get("$num") {
                    return Val::Num(match s.as_str() {
                        "inf" => f64::INFINITY,
                        "-inf" => f64::NEG_INFINITY,
                        _ => f64::NAN,
                    });
                }
            }
            let t = Table::new();
            for (k, x) in o {
                match k.parse::<i64>() {
                    Ok(i) => t.set(i, val_from_json(x)),
                    Err(_) => t.set(Name::from(k.as_str()), val_from_json(x)),
                }
            }
            Val::Table(t)
        }
    }
}

thread_local! {
    /// Set when [`mod_from_json`] dropped a tag key `pob_gamedata::Tag` does not model.
    static STRIPPED: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// Tag keys PoB writes that the frozen `pob_gamedata::Tag` has no field for
/// (`warcryPowerBonus`, CalcPerform.lua:2430; MS-A's CCR-0014 reports it).
const UNMODELLED_TAG_KEYS: [&str; 1] = ["warcryPowerBonus"];

/// The oracle's mod JSON (`tags` array) in the dump shape `pob_gamedata::Mod` reads (`$seq`).
fn mod_from_json(v: &serde_json::Value) -> Mod {
    fn rename(v: &serde_json::Value) -> serde_json::Value {
        match v {
            serde_json::Value::Object(o) => {
                let is_mod =
                    o.contains_key("name") && o.contains_key("type") && o.contains_key("tags");
                serde_json::Value::Object(
                    o.iter()
                        .filter(|(k, _)| {
                            let drop = UNMODELLED_TAG_KEYS.contains(&k.as_str());
                            if drop {
                                STRIPPED.with(|c| c.set(true));
                            }
                            !drop
                        })
                        .map(|(k, x)| {
                            let k = if is_mod && k == "tags" {
                                "$seq".to_owned()
                            } else {
                                k.clone()
                            };
                            (k, rename(x))
                        })
                        .collect(),
                )
            }
            serde_json::Value::Array(a) => serde_json::Value::Array(a.iter().map(rename).collect()),
            other => other.clone(),
        }
    }
    serde_json::from_value(rename(v)).unwrap_or_else(|e| panic!("mod JSON {v}: {e}"))
}

/// `exportBuffs` text as a multiset of blocks (PoB writes the buffs in `pairs` order).
fn export_blocks(ty: &str, s: &str) -> Vec<String> {
    let mut s = s.to_owned();
    let mut out = Vec::new();
    if ty == "Curse" {
        // "--- Curse Limit ---\n<limit>" comes first
        let mut it = s.splitn(3, '\n');
        let h1 = it.next().unwrap_or("").to_owned();
        let h2 = it.next().unwrap_or("").to_owned();
        out.push(format!("{h1}\n{h2}"));
        s = it.next().unwrap_or("").to_owned();
    }
    if matches!(ty, "Aura" | "Curse" | "Warcry" | "Link") {
        for b in s.split("\n---") {
            let b = b.trim_start_matches('\n');
            if !b.is_empty() {
                out.push(b.to_owned());
            }
        }
    } else {
        out.extend(s.split('\n').filter(|l| !l.is_empty()).map(str::to_owned));
    }
    out.sort();
    out
}

const EXPORT_TYPES: [&str; 7] = [
    "PlayerMods",
    "Aura",
    "Curse",
    "Warcry",
    "Link",
    "EnemyConditions",
    "EnemyMods",
];

/// The Rust half of bmb.lua's `exportStage`: ParseBuffs of PoB's exported strings into a fresh
/// party tab, and exportBuffs of the injected `buffExports`.
fn rust_export_stage(data: &'static Data, lua: &LuaDump, r: &mut Report) -> Pending {
    let before = pob_calc::stub::stub_report();
    let mut d = Dump::default();
    let strs: BTreeMap<&str, String> = EXPORT_TYPES
        .iter()
        .map(|ty| {
            let s = lua
                .lines
                .get(&format!("str.{ty}"))
                .cloned()
                .unwrap_or_default();
            let s = s.strip_prefix("s:").unwrap_or("").to_owned();
            (*ty, unesc(&s))
        })
        .collect();
    let build = Build::new(data);
    let pt = &build.partyTab;
    pt.ctor_model();
    let labels: BTreeMap<&str, Table> = LABELS
        .iter()
        .map(|c| {
            let t = Table::new();
            t.set(n!("label"), "");
            (*c, t)
        })
        .collect();
    let modDB = pt.actor.modDB.get();
    let output = pt.actor.output.get();
    let eml = pt.enemyModList.get();
    let runs: Vec<(&str, Box<dyn Fn()>)> = vec![
        (
            "PartyMemberStats",
            Box::new(|| {
                pt.ParseBuffs(
                    &build,
                    BuffList::Store(&modDB),
                    &strs["PlayerMods"],
                    n!("PartyMemberStats"),
                    Some(&output),
                )
            }),
        ),
        (
            "Aura",
            Box::new(|| {
                pt.ParseBuffs(
                    &build,
                    BuffList::Table(&pt.actorList(n!("Aura"))),
                    &strs["Aura"],
                    n!("Aura"),
                    Some(&labels["simpleAuras"]),
                )
            }),
        ),
        (
            "Curse",
            Box::new(|| {
                pt.ParseBuffs(
                    &build,
                    BuffList::Table(&pt.actorList(n!("Curse"))),
                    &strs["Curse"],
                    n!("Curse"),
                    Some(&labels["simpleCurses"]),
                )
            }),
        ),
        (
            "Warcry",
            Box::new(|| {
                pt.ParseBuffs(
                    &build,
                    BuffList::Table(&pt.actorList(n!("Warcry"))),
                    &strs["Warcry"],
                    n!("Warcry"),
                    Some(&labels["simpleWarcries"]),
                )
            }),
        ),
        (
            "Link",
            Box::new(|| {
                pt.ParseBuffs(
                    &build,
                    BuffList::Table(&pt.actorList(n!("Link"))),
                    &strs["Link"],
                    n!("Link"),
                    Some(&labels["simpleLinks"]),
                )
            }),
        ),
        (
            "EnemyConditions",
            Box::new(|| {
                pt.ParseBuffs(
                    &build,
                    BuffList::Store(&eml),
                    &strs["EnemyConditions"],
                    n!("EnemyConditions"),
                    None,
                )
            }),
        ),
        (
            "EnemyMods",
            Box::new(|| {
                pt.ParseBuffs(
                    &build,
                    BuffList::Store(&eml),
                    &strs["EnemyMods"],
                    n!("EnemyMods"),
                    Some(&labels["simpleEnemyMods"]),
                )
            }),
        ),
    ];
    for (ty, f) in runs {
        if let Err(e) = catch_lua(ty, || f()) {
            d.emit(format!("perr.{ty}"), e.as_str());
        }
    }
    let _ = catch_lua("party dump", || {
        for k in ["Aura", "Curse", "Warcry", "Link"] {
            dump_buff_list(&mut d, &format!("party.{k}"), &pt.actorList(Name::from(k)));
        }
        dump_store(&mut d, "party.modDB", &modDB);
        dump_output(&mut d, "party.output", &output);
        dump_store(&mut d, "party.enemyModList", &eml);
        for c in LABELS {
            if c != "simpleEnemyCond" {
                d.emit(format!("party.label.{c}"), labels[c].get(n!("label")));
            }
        }
    });

    // exportBuffs of the injected buffExports
    let build2 = Build::new(data);
    let pt2 = &build2.partyTab;
    pt2.ctor_model();
    pt2.enableExportBuffs.set(true);
    // per type, noting the types whose mods lost an unmodelled tag key
    let mut stripped: BTreeSet<String> = BTreeSet::new();
    let exports = Table::new();
    if let Some(serde_json::Value::Object(o)) = lua.json.get("exports") {
        for (ty, v) in o {
            STRIPPED.with(|c| c.set(false));
            let val = val_from_json(v);
            if STRIPPED.with(std::cell::Cell::get) {
                stripped.insert(ty.clone());
            }
            match ty.parse::<i64>() {
                Ok(i) => exports.set(i, val),
                Err(_) => exports.set(Name::from(ty.as_str()), val),
            }
        }
    }
    pt2.buffExports.set(Some(exports));
    let mut exp = Dump::default();
    for ty in EXPORT_TYPES {
        let s = match catch_lua(ty, || pt2.exportBuffs(Name::from(ty))) {
            Ok(s) => s,
            Err(e) => format!("ERROR {e}"),
        };
        let want = strs[ty].clone();
        let (a, b) = (export_blocks(ty, &want), export_blocks(ty, &s));
        exp.emit(
            format!("str.{ty}"),
            if a == b { "same" } else { "different" },
        );
        if a != b {
            exp.emit(format!("str.{ty}.rust"), s.as_str());
        }
    }
    let hits = stub_hits_since(&before);
    let pending = Pending::from_hits(&hits);

    // perr: compare presence only (messages differ)
    let mut lua_lines = lua.lines.clone();
    for (k, v) in lua_lines.iter_mut() {
        if k.starts_with("perr.") {
            *v = "s:error".to_owned();
        }
        if k.starts_with("str.") {
            *v = "s:same".to_owned();
        }
    }
    for (k, v) in d.lines.iter_mut() {
        if k.starts_with("perr.") {
            *v = "s:error".to_owned();
        }
    }
    let mut rust_lines = d.lines;
    rust_lines.extend(exp.lines.into_iter().filter(|(k, _)| !k.ends_with(".rust")));
    let gap = |k: &str| k.strip_prefix("str.").is_some_and(|t| stripped.contains(t));
    if !stripped.is_empty() {
        r.notes.insert(format!(
            "exportBuffs of {stripped:?}: injected mods lost the tag key warcryPowerBonus, which \
             pob_gamedata::Tag does not model (MS-A CCR-0014); counted as pending"
        ));
    }
    compare_t(
        "export",
        &lua_lines,
        &rust_lines,
        &["party.", "perr.", "str."],
        &|k| pending.key(k) || gap(k),
        &|k| lua_order_label(k, &lua_lines, &rust_lines),
        r,
    );
    pending
}

fn unesc(s: &str) -> String {
    let b = s.as_bytes();
    let mut o = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'\\' && i + 3 < b.len() && b[i + 1..i + 4].iter().all(u8::is_ascii_digit) {
            o.push(
                std::str::from_utf8(&b[i + 1..i + 4])
                    .unwrap()
                    .parse::<u8>()
                    .unwrap(),
            );
            i += 4;
        } else {
            o.push(b[i]);
            i += 1;
        }
    }
    String::from_utf8_lossy(&o).into_owned()
}

fn sections(lua: &LuaDump) -> BTreeMap<String, XmlNode> {
    lua.json
        .iter()
        .filter_map(|(k, v)| {
            k.strip_prefix("xml.")
                .map(|e| (e.to_owned(), xml_from_json(v)))
        })
        .collect()
}

/// Compare one oracle capture (sections loaded at Build:Init) with the Rust tabs: state, Save,
/// and the Rust Save → Load round trip.
fn check_capture(
    what: &str,
    data: &'static Data,
    lua: &LuaDump,
    r: &mut Report,
) -> Result<Pending, String> {
    let before = pob_calc::stub::stub_report();
    let xml = sections(lua);
    let build = new_build(data, &lua.lines);
    let loaded = rust_load(&build, &xml);
    let hits = stub_hits_since(&before);
    let pending = Pending::from_hits(&hits);
    let d = match loaded {
        Ok(d) => d,
        // validateGemLevel (CT) leaves an invalid level in place; the next `levels[level]` raises
        Err(e) if pending.gemLevel => {
            r.compared += 1;
            r.pending += 1;
            if r.pending_examples.len() < 8 {
                r.pending_examples.push(format!("{what}: {e}"));
            }
            return Ok(pending);
        }
        Err(e) => return Err(e),
    };
    // ConfigTab:BuildModList (CO-C) is a stub: compare with PoB's config as Load left it,
    // before its BuildModList ran (bmb.lua `cfgpre.*`, `save.ConfigPre`)
    let preCfg = hits.contains("Classes/ConfigTab.lua:1130-1203");
    let mut prefixes = Vec::new();
    for (sec, p) in [
        ("Skills", "skills."),
        ("Config", "cfg."),
        ("Party", "party."),
    ] {
        if xml.contains_key(sec) {
            prefixes.push(p);
        }
    }
    let mut lua_lines: Lines = Lines::new();
    for (k, v) in &lua.lines {
        if k.starts_with("cfg.defaultState.") {
            continue;
        }
        if preCfg {
            if k.starts_with("cfg.") {
                continue;
            }
            if let Some(rest) = k.strip_prefix("cfgpre.") {
                lua_lines.insert(format!("cfg.{rest}"), v.clone());
                continue;
            }
        }
        lua_lines.insert(k.clone(), v.clone());
    }
    compare_t(
        what,
        &lua_lines,
        &d.lines,
        &prefixes,
        &|k| pending.key(k),
        &|k| {
            lua_order_gem(data, k, &lua_lines, &d.lines) || lua_order_label(k, &lua_lines, &d.lines)
        },
        r,
    );

    // defaultState
    if lua.lines.keys().any(|k| k.starts_with("cfg.defaultState.")) {
        let mut ds = Dump::default();
        for (k, v) in defaultState(data).pairs() {
            ds.emit(format!("cfg.defaultState.{}", key_str(&k)), v);
        }
        compare(
            what,
            &lua.lines,
            &ds.lines,
            &["cfg.defaultState."],
            &|_| false,
            r,
        );
    }

    // Save (node for node; config children as a multiset: PoB writes `pairs` order)
    for (sec, unordered) in [("Skills", false), ("Config", true), ("Party", false)] {
        if !xml.contains_key(sec) {
            continue;
        }
        let key = if sec == "Config" && preCfg {
            "save.ConfigPre".to_owned()
        } else {
            format!("save.{sec}")
        };
        let Some(want) = lua.json.get(&key) else {
            r.mismatches
                .push(format!("{what}: no {key} from the oracle"));
            continue;
        };
        let mut wantNode = xml_from_json(want);
        let mut gotNode = rust_save(&build, sec)?;
        r.tolerated += mask_ambiguous(data, &mut wantNode);
        mask_ambiguous(data, &mut gotNode);
        let want = canon(&wantNode, unordered, 0);
        let got = canon(&gotNode, unordered, 0);
        r.compared += 1;
        if want != got {
            let msg = format!("{what}: {key} differs\n  lua:  {want}\n  rust: {got}");
            let isPending = match sec {
                "Skills" => pending.gemReq || pending.gemLevel,
                "Party" => pending.party,
                _ => false,
            };
            if isPending {
                r.pending += 1;
                if r.pending_examples.len() < 8 {
                    r.pending_examples.push(msg);
                }
            } else {
                r.mismatches.push(msg);
            }
        }
    }

    // Round trip: Rust Save → Rust Load into a fresh build. A nil includeInFullDPS is saved
    // as "nil" and loads back as false, in PoB as well.
    let mut saved = BTreeMap::new();
    for sec in ["Skills", "Config", "Party"] {
        if xml.contains_key(sec) {
            saved.insert(sec.to_owned(), rust_save(&build, sec)?);
        }
    }
    let build2 = new_build(data, &lua.lines);
    let d2 = rust_load(&build2, &saved)?;
    let mut first = d.lines.clone();
    for (k, v) in &d2.lines {
        if k.ends_with(".includeInFullDPS") && v == "b:false" && !first.contains_key(k) {
            first.insert(k.clone(), v.clone());
        }
    }
    // Save leaves out an input equal to its default state (ConfigTab.lua:971), so it does not come back
    let droppedDefault = |k: &str| -> bool {
        let Some(rest) = k.strip_prefix("cfg.sets[") else {
            return false;
        };
        let Some((_, var)) = rest.split_once("].input.") else {
            return false;
        };
        if d2.lines.contains_key(k) {
            return false;
        }
        let Some(v) = first.get(k) else { return false };
        let ty = match v.split_once(':').map(|(t, _)| t) {
            Some("n") => "number",
            Some("b") => "boolean",
            Some("s") => "string",
            _ => return false,
        };
        enc(&build
            .configTab
            .GetDefaultState(Name::from(var), Name::from(ty)))
        .as_ref()
            == Some(v)
    };
    compare_t(
        &format!("{what} round trip"),
        &first,
        &d2.lines,
        &prefixes,
        &|_| false,
        &droppedDefault,
        r,
    );
    Ok(pending)
}

fn report(name: &str, r: &Report, pending_units: &BTreeSet<&str>) {
    eprintln!(
        "{name}: {} values compared, {} mismatches, {} pending, {} LUA-ORDER/expected{}",
        r.compared,
        r.mismatches.len(),
        r.pending,
        r.tolerated,
        if pending_units.is_empty() {
            String::new()
        } else {
            format!(
                " (unported: {})",
                pending_units.iter().copied().collect::<Vec<_>>().join(", ")
            )
        }
    );
    for n in &r.notes {
        eprintln!("  note: {n}");
    }
    for p in &r.pending_examples {
        eprintln!("  pending: {p}");
    }
    for m in r.mismatches.iter().take(60) {
        eprintln!("  MISMATCH {m}");
    }
}

/// The corpus size (`.parity/builds/*.xml`).
const CORPUS_BUILDS: usize = 57;
/// Coverage floors and bucket bounds (HYG2/V2): `(min compared, max pending, max
/// LUA-ORDER/expected)` per report. The floors and pending counts are the last green runs on
/// rust-port (148,776 / 3310 / 1302 values, 0 / 5 / 0 pending; on 746b103 this test is red
/// before reaching them, see docs/port/FOLLOWUPS.md "HYG2 exposed"). The LUA-ORDER counts are
/// PoB's own `pairs` order, which varies between processes: observed 12-13 (tabs), 26-28
/// (buffs) and 1 (synthetic) over two runs; the bounds leave about 25 % headroom for that.
const TABS_BOUNDS: (usize, usize, usize) = (148_776, 0, 16);
/// ParseBuffs/exportBuffs: the 5 pending values are `str.Warcry` exports whose injected mods
/// lose the tag key `warcryPowerBonus` (pob_gamedata::Tag does not model it, MS-A CCR-0014).
const BUFFS_BOUNDS: (usize, usize, usize) = (3310, 5, 34);
const SYNTHETIC_BOUNDS: (usize, usize, usize) = (1302, 0, 2);

/// A report's coverage and its pending / tolerated buckets stay within `(min compared,
/// max pending, max tolerated)`.
#[track_caller]
fn assert_bounds(
    name: &str,
    r: &Report,
    (min_compared, max_pending, max_tolerated): (usize, usize, usize),
) {
    assert!(
        r.compared >= min_compared,
        "{name}: {} values compared, expected at least {min_compared}",
        r.compared
    );
    assert!(
        r.pending <= max_pending,
        "{name}: {} pending values, bound {max_pending}",
        r.pending
    );
    assert!(
        r.tolerated <= max_tolerated,
        "{name}: {} LUA-ORDER/expected values, bound {max_tolerated}",
        r.tolerated
    );
}

fn boot(src: &Path) -> JitPob {
    let mut pob =
        JitPob::boot_oracle(src, Some(QUIET_PRELUDE)).unwrap_or_else(|e| panic!("PoB boot: {e}"));
    pob.run(ORACLE_LUA)
        .unwrap_or_else(|e| panic!("oracle.lua: {e}"));
    pob.run(BMB_LUA).unwrap_or_else(|e| panic!("bmb.lua: {e}"));
    pob
}

fn first_text(pob: &mut JitPob, code: &str) -> String {
    let r = pob.run(code).unwrap_or_else(|e| panic!("{code}: {e}"));
    r.first().map(|r| r.text().to_owned()).unwrap_or_default()
}

fn note_pending(units: &mut BTreeSet<&'static str>, p: &Pending) {
    if p.gemReq || p.gemLevel {
        units.insert("CT");
    }
    if p.party {
        units.insert("CT/MS-A");
    }
}

#[test]
fn corpus_tabs_match_oracle() {
    let _serial = SERIAL
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let Some(src) = pob_testpaths::require(
        find_pob_src(),
        "PoB checkout (.PathOfBuilding/src) not found",
    ) else {
        return; // POB_ALLOW_SKIP=1
    };
    let Some(data) = pob_testpaths::require(init_data(), "data/dump/data.bin not found") else {
        return; // POB_ALLOW_SKIP=1
    };
    let Some(parity) = pob_testpaths::require(find_parity_dir(), ".parity not found") else {
        return; // POB_ALLOW_SKIP=1
    };
    let mut builds: Vec<PathBuf> = std::fs::read_dir(parity.join("builds"))
        .map(|rd| rd.filter_map(|e| e.ok().map(|e| e.path())).collect())
        .unwrap_or_default();
    builds.retain(|p| p.extension().is_some_and(|e| e == "xml"));
    let filtered = std::env::var("BMB_BUILDS").ok();
    if let Some(f) = &filtered {
        builds.retain(|p| p.to_string_lossy().contains(f.as_str()));
    } else if !pob_testpaths::quick_corpus() {
        assert_eq!(builds.len(), CORPUS_BUILDS, ".parity/builds/*.xml count");
    }
    builds.sort();
    if builds.is_empty() {
        let _ = pob_testpaths::require(None::<()>, ".parity/builds/*.xml not found");
        return; // POB_ALLOW_SKIP=1
    }
    let mut r = Report::default();
    let mut rx = Report::default();
    let mut units = BTreeSet::new();
    let mut groups = 0usize;
    let mut exported = 0usize;
    for b in &builds {
        let stem = b.file_stem().unwrap().to_string_lossy().into_owned();
        let xml = std::fs::read_to_string(b).unwrap();
        let (cap, exp) = {
            let _lock = OracleLock::acquire().unwrap_or_else(|e| panic!("oracle lock: {e}"));
            let mut pob = boot(&src);
            pob.load_build_xml(&xml, &stem)
                .unwrap_or_else(|e| panic!("{stem}: {e}"));
            let cap = first_text(&mut pob, "return __bmb.captured or 'error\\tno capture'");
            let exp = first_text(&mut pob, "return __bmb.exportStage()");
            (cap, exp)
        };
        let lua = parse_lua(&cap);
        groups += lua
            .lines
            .get("skills.groups.n")
            .and_then(|v| num_of(v))
            .unwrap_or(0.0) as usize;
        match check_capture(&stem, data, &lua, &mut r) {
            Ok(p) => note_pending(&mut units, &p),
            Err(e) => r.mismatches.push(format!("{stem}: {e}")),
        }
        let lexp = parse_lua(&exp);
        if EXPORT_TYPES.iter().any(|t| {
            lexp.lines
                .get(&format!("str.{t}"))
                .is_some_and(|s| s != "s:")
        }) {
            exported += 1;
        }
        let p = rust_export_stage(data, &lexp, &mut rx);
        note_pending(&mut units, &p);
        let _ = &stem;
    }
    eprintln!(
        "BM-B corpus: {} builds, {} socket groups (active set), {} builds with party exports",
        builds.len(),
        groups,
        exported
    );
    report("tabs (state, Save, round trip)", &r, &units);
    report("ParseBuffs/exportBuffs", &rx, &units);
    assert!(
        r.mismatches.is_empty() && rx.mismatches.is_empty(),
        "BM-B corpus mismatches"
    );
    if filtered.is_none() && !pob_testpaths::quick_corpus() {
        assert_bounds("tabs", &r, TABS_BOUNDS);
        assert_bounds("ParseBuffs/exportBuffs", &rx, BUFFS_BOUNDS);
    } else {
        // a restricted run still bounds the buckets
        assert_bounds("tabs", &r, (1, TABS_BOUNDS.1, TABS_BOUNDS.2));
        assert_bounds(
            "ParseBuffs/exportBuffs",
            &rx,
            (0, BUFFS_BOUNDS.1, BUFFS_BOUNDS.2),
        );
    }
}

#[test]
fn synthetic_sections_match_oracle() {
    let _serial = SERIAL
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let Some(src) = pob_testpaths::require(
        find_pob_src(),
        "PoB checkout (.PathOfBuilding/src) not found",
    ) else {
        return; // POB_ALLOW_SKIP=1
    };
    let Some(data) = pob_testpaths::require(init_data(), "data/dump/data.bin not found") else {
        return; // POB_ALLOW_SKIP=1
    };
    let Some(parity) = pob_testpaths::require(find_parity_dir(), ".parity not found") else {
        return; // POB_ALLOW_SKIP=1
    };
    let base = parity.join("builds/upstream_OccVortex.xml");
    if pob_testpaths::require(
        base.is_file().then_some(()),
        ".parity/builds/upstream_OccVortex.xml not found",
    )
    .is_none()
    {
        return; // POB_ALLOW_SKIP=1
    }
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("corpus/BM-B");
    let mut files: Vec<PathBuf> = std::fs::read_dir(&dir)
        .unwrap()
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| {
            p.file_name()
                .is_some_and(|n| n.to_string_lossy().starts_with("synthetic_"))
        })
        .collect();
    files.sort();
    assert!(
        files.len() >= 3,
        "synthetic fixtures missing in {}",
        dir.display()
    );
    let mut r = Report::default();
    let mut units = BTreeSet::new();
    for f in &files {
        let stem = f.file_stem().unwrap().to_string_lossy().into_owned();
        let text = std::fs::read_to_string(f).unwrap();
        let out = {
            let _lock = OracleLock::acquire().unwrap_or_else(|e| panic!("oracle lock: {e}"));
            let mut pob = boot(&src);
            pob.load_build_xml(&std::fs::read_to_string(&base).unwrap(), "base")
                .unwrap_or_else(|e| panic!("base build: {e}"));
            let quoted = format!("{text:?}");
            // a Rust string literal is a valid Lua long-escape string for this ASCII text
            first_text(&mut pob, &format!("return __bmb.syntheticStage({quoted})"))
        };
        let lua = parse_lua(&out);
        match check_capture(&stem, data, &lua, &mut r) {
            Ok(p) => note_pending(&mut units, &p),
            Err(e) => r.mismatches.push(format!("{stem}: {e}")),
        }
    }
    report(&format!("synthetic ({} files)", files.len()), &r, &units);
    assert!(r.mismatches.is_empty(), "BM-B synthetic mismatches");
    assert_bounds("synthetic", &r, SYNTHETIC_BOUNDS);
}

/// `SkillsTab:ProcessGemLevel` (SkillsTab.lua:1129-1157) for every gem, every default gem
/// level mode (plus an unknown one), five character levels, imbued or not.
#[test]
fn process_gem_level_matches_oracle() {
    let _serial = SERIAL
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let Some(src) = pob_testpaths::require(
        find_pob_src(),
        "PoB checkout (.PathOfBuilding/src) not found",
    ) else {
        return; // POB_ALLOW_SKIP=1
    };
    let Some(data) = pob_testpaths::require(init_data(), "data/dump/data.bin not found") else {
        return; // POB_ALLOW_SKIP=1
    };
    let Some(parity) = pob_testpaths::require(find_parity_dir(), ".parity not found") else {
        return; // POB_ALLOW_SKIP=1
    };
    let base = parity.join("builds/upstream_OccVortex.xml");
    if pob_testpaths::require(
        base.is_file().then_some(()),
        ".parity/builds/upstream_OccVortex.xml not found",
    )
    .is_none()
    {
        return; // POB_ALLOW_SKIP=1
    }
    let out = {
        let _lock = OracleLock::acquire().unwrap_or_else(|e| panic!("oracle lock: {e}"));
        let mut pob = boot(&src);
        pob.load_build_xml(&std::fs::read_to_string(&base).unwrap(), "base")
            .unwrap_or_else(|e| panic!("base build: {e}"));
        first_text(&mut pob, "return __bmb.processGemLevelStage()")
    };
    let lua = parse_lua(&out);
    let build = Build::new(data);
    let mut d = Dump::default();
    for mode in [
        "normalMaximum",
        "corruptedMaximum",
        "awakenedMaximum",
        "characterLevel",
        "levelOne",
        "bogus",
    ] {
        build.skillsTab.defaultGemLevel.set(Some(Name::from(mode)));
        for cl in [1.0, 12.0, 38.0, 70.0, 100.0] {
            build.characterLevel.set(cl);
            for (gemId, gemData) in &data.gems {
                for imbued in [false, true] {
                    let v = match catch_lua("ProcessGemLevel", || {
                        build.skillsTab.ProcessGemLevel(&build, gemData, imbued)
                    }) {
                        Ok(v) => Val::Num(v),
                        Err(_) => Val::from("ERROR"),
                    };
                    d.emit(
                        format!("pgl.{mode}.{}.{imbued}.{gemId}", tostring_num(cl)),
                        v,
                    );
                }
            }
        }
    }
    let mut r = Report::default();
    compare(
        "ProcessGemLevel",
        &lua.lines,
        &d.lines,
        &["pgl."],
        &|_| false,
        &mut r,
    );
    report("ProcessGemLevel", &r, &BTreeSet::new());
    assert!(r.compared > 50_000, "too few values: {}", r.compared);
    assert!(r.mismatches.is_empty(), "ProcessGemLevel mismatches");
}

/// `PartyTab:setBuffExports` (PartyTab.lua:984-990): ignored unless export is enabled; otherwise
/// the old table is wiped and a shallow copy installed; `exportBuffs` then consumes an entry.
#[test]
fn set_buff_exports() {
    let _serial = SERIAL
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let Some(data) = pob_testpaths::require(init_data(), "data/dump/data.bin not found") else {
        return; // POB_ALLOW_SKIP=1
    };
    let build = Build::new(data);
    let pt = &build.partyTab;
    pt.ctor_model();
    let old = pt.buffExports.get().expect("ctor_model sets { }");
    old.set(n!("Aura"), Table::new());
    let fresh = Table::new();
    let aura = Table::new();
    fresh.set(n!("Aura"), aura.clone());
    fresh.set(n!("CurseLimit"), 2.0);
    // disabled: nothing changes
    pt.setBuffExports(&fresh);
    assert!(pt.buffExports.get().unwrap().ptr_eq(&old));
    assert!(old.truthy(n!("Aura")));
    // enabled: the old table is wiped, the new one is a copy sharing its sub-tables (noRecurse)
    pt.enableExportBuffs.set(true);
    pt.setBuffExports(&fresh);
    assert!(!old.truthy(n!("Aura")), "old table wiped");
    let cur = pt.buffExports.get().unwrap();
    assert!(!cur.ptr_eq(&fresh));
    assert!(cur.table(n!("Aura")).unwrap().ptr_eq(&aura));
    assert_eq!(cur.num(n!("CurseLimit")), 2.0);
    // no Curse entry: ""; an empty Aura entry exports "" and becomes { ConvertedToText = true, string = "" }
    assert_eq!(pt.exportBuffs(n!("Curse")), "");
    assert_eq!(pt.exportBuffs(n!("Aura")), "");
    let conv = cur.table(n!("Aura")).unwrap();
    assert!(!conv.ptr_eq(&aura));
    assert!(conv.truthy(n!("ConvertedToText")));
    assert_eq!(pt.exportBuffs(n!("Aura")), "");
}

/// The entry points other code calls must work on a fresh `Build` whose tabs never ran Load
/// (a new build, an XML without `<Party>`/`<Config>`): PoB's constructors set their state
/// (PartyTab.lua:25, ConfigTab.lua:145-148, SkillsTab.lua:95; CCR-0019 §10).
#[test]
fn fresh_build_entry_points() {
    let _serial = SERIAL
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let Some(data) = pob_testpaths::require(init_data(), "data/dump/data.bin not found") else {
        return; // POB_ALLOW_SKIP=1
    };
    // setBuffExports with export enabled and no prior ctor_model (ImportTab.lua:508/748 →
    // CalcPerform.lua:1312)
    let build = Build::new(data);
    let pt = &build.partyTab;
    assert!(pt.buffExports.get().is_none(), "Build::new leaves the field unset");
    pt.enableExportBuffs.set(true);
    let fresh = Table::new();
    let aura = Table::new();
    fresh.set(n!("Aura"), aura.clone());
    pt.setBuffExports(&fresh);
    let cur = pt.buffExports.get().expect("installed");
    assert!(!cur.ptr_eq(&fresh));
    assert!(cur.table(n!("Aura")).unwrap().ptr_eq(&aura));
    assert_eq!(pt.exportBuffs(n!("Curse")), "");
    // disabled export on a fresh build: no error, constructor's `{ }`
    let build = Build::new(data);
    build.partyTab.setBuffExports(&fresh);
    let cur = build.partyTab.buffExports.get().expect("ctor `{ }`");
    assert!(cur.pairs().next().is_none());
    // exportBuffs on a fresh build
    let build = Build::new(data);
    build.partyTab.enableExportBuffs.set(true);
    assert_eq!(build.partyTab.exportBuffs(n!("Aura")), "");
    // GetDefaultState: the constructor's config set 1 (NewConfigSet placeholders)
    let build = Build::new(data);
    let ct = &build.configTab;
    assert!(ct
        .GetDefaultState(Name::from("noSuchVar"), n!("number"))
        .raw_eq(&Val::Num(0.0)));
    assert_eq!(ct.activeConfigSetId.get(), 1.0);
    assert!(ct.configSet(1.0).is_some());
    assert!(ct
        .configSet(1.0)
        .unwrap()
        .table(n!("input"))
        .unwrap()
        .ptr_eq(&ct.input));
    for v in &data.configOptions {
        let Some(var) = v.var else { continue };
        if let Some(p) = &v.defaultPlaceholderState {
            assert!(
                ct.GetDefaultState(var, n!("nil")).raw_eq(&Val::from_value(p)),
                "placeholder default of {var}"
            );
        }
    }
    // ProcessGemLevel with defaultGemLevel unset: the constructor's "normalMaximum"
    let build = Build::new(data);
    assert!(build.skillsTab.defaultGemLevel.get().is_none());
    build.characterLevel.set(1.0);
    let mut n = 0;
    for gemData in data.gems.values() {
        let a = build.skillsTab.ProcessGemLevel(&build, gemData, false);
        assert_eq!(a, gemData.naturalMaxLevel, "{}", gemData.id);
        n += 1;
    }
    assert!(n > 100);
}
