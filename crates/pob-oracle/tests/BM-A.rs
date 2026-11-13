//! Unit BM-A acceptance (calc doc §11.1, row BM-A): build XML load/save and the items, tree
//! and calcs tab models against PoB under LuaJIT (the oracle, `jit.off()`).
//!
//! For every `.parity/builds/*.xml` build (`bm_a_corpus`):
//! * **load** — `loadBuildModelFromXML` (Build:Init's model half + LoadDB, then
//!   `itemsTab:UpdateSockets()` as the oracle's load script does) gives the same model as PoB
//!   after its load: the Build section, every item (name, base, rarity, type, raw text,
//!   variants, mod-line ranges), item sets, every slot (selected item, active/inactive, weapon
//!   set, node id, label, parent, valid items), sockets, trade weights, every spec (version,
//!   class ids, allocated nodes, jewels, masteries), the calcs tab input and section state,
//!   and the skills/config set skeleton (`corpus/BM-A/bma.lua` writes PoB's side);
//! * **save vs PoB** — the Rust `SaveDB` text equals PoB's own `SaveDB` text once both are
//!   parsed and canonicalised (attributes and children compared as multisets: PoB writes
//!   `pairs` order). Display-stat elements (`PlayerStat`, `FullDPSSkill`, `MinionStat`) and the
//!   UI-only sections (`Notes`, `TreeView`, `Import`) are left out (CCR-0080 §8);
//! * **round trip** — Rust `SaveDB` → Rust load gives the same model as PoB loading that
//!   Rust-saved XML (a save of a legacy build is not a fixed point of load, in PoB either:
//!   regenerated item text, defaults written out), and saving again gives the same XML (canonicalised; the spec's node / mastery lists and the tree URL
//!   follow `pairs(allocNodes)`, whose order depends on insertion history, in PoB as well);
//! * **PoB on the Rust-saved XML** — PoB loading the Rust-saved XML computes exactly the same
//!   output tables (`__oracle_outputs`: player and minion) as PoB loading the original.
//!
//! The Rust model is compared with PoB's at the first `calcsTab:BuildOutput()` of the load
//! (bma.lua hooks it; `loadBuildModelFromXML` stops there), PoB's `SaveDB` is taken at the same
//! point, and `UpdateSockets` (which the oracle's load script runs after the load) is checked
//! against PoB's state after it. Every difference fails, on every build (cluster-jewel
//! subgraphs are PS-C's `BuildClusterJewelGraphs`, the placeholders CO-C's `BuildModList`,
//! both real), with one bucket that empties itself: while PS-B's `BuildAllDependsAndPaths`
//! (PassiveSpec.lua:1092-1695) is an `unported!` stub *and the build's Rust load hit it*, a
//! spec's allocated set that is a strict superset of PoB's (PoB's pass deallocates nodes that
//! are not connected, ignored, or masteries without an effect; the stub removes nothing), and a
//! `<Tree>` section that is equal once each Spec's `nodes` list (Rust's a superset of PoB's) and
//! tree URL are left out, count as *pending PS-B*. Nothing else is excused; PoB's outputs on the
//! Rust-saved XML are always compared (PoB's own pass prunes the extra nodes again).
//!
//! A test whose inputs (data/dump, the PoB checkout, .parity/builds) are absent skips with a
//! message; with `POB_ORACLE_REQUIRE=1` (or `CI`) it fails instead. `BMA_BUILDS=<substring>`
//! restricts the corpus; `BMA_SAVE_DIR=<dir>` writes the Rust- and PoB-saved XML there.
#![allow(non_snake_case, clippy::pedantic)]

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use pob_calc::model::build_xml::{characterLevelAutoMode, loadBuildModelFromXML, timelessData};
use pob_calc::model::xml::ParseXML;
use pob_calc::model::{BuildRef, XmlChild, XmlNode};
use pob_calc::prelude::*;
use pob_gamedata::timeless_lut as lut;
use pob_oracle::jit::JitPob;
use pob_oracle::oracle::{
    find_parity_dir, find_pob_src, find_up, OracleOutput, ORACLE_LUA, QUIET_PRELUDE,
};

const BMA_LUA: &str = include_str!("../corpus/BM-A/bma.lua");

static SERIAL: Mutex<()> = Mutex::new(());

fn skip(why: &str) {
    let required = std::env::var_os("POB_ORACLE_REQUIRE").is_some_and(|v| v == "1")
        || std::env::var_os("CI").is_some_and(|v| !v.is_empty() && v != "false" && v != "0");
    assert!(!required, "required input missing: {why}");
    eprintln!("SKIPPED: {why}");
}

fn init_data() -> Option<&'static Data> {
    let dir = if let Some(p) = std::env::var_os("POB_DUMP_DIR").map(PathBuf::from) {
        p.join("data.bin").is_file().then_some(p)?
    } else {
        find_up("data/dump/data.bin")?.parent()?.to_path_buf()
    };
    pob_gamedata::init_from_dir(&dir).unwrap_or_else(|e| panic!("data.bin: {e}"));
    Some(pob_gamedata::data())
}

fn quiet_lua_errors() {
    let prev = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        if info.payload().downcast_ref::<LuaError>().is_none() {
            prev(info);
        }
    }));
}

fn catch<R>(f: impl FnOnce() -> R) -> Result<R, String> {
    match std::panic::catch_unwind(std::panic::AssertUnwindSafe(f)) {
        Ok(r) => Ok(r),
        Err(payload) => match payload.downcast::<LuaError>() {
            Ok(e) => Err(e.0),
            Err(other) => std::panic::resume_unwind(other),
        },
    }
}

// ---- the model dump (the Rust twin of bma.lua) ---------------------------------------------

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

fn opt_num(v: Option<f64>) -> Val {
    v.map_or(Val::Nil, Val::Num)
}
fn nan_nil(v: f64) -> Val {
    if v.is_nan() {
        Val::Nil
    } else {
        Val::Num(v)
    }
}
fn opt_str(v: Option<impl AsRef<str>>) -> Val {
    v.map_or(Val::Nil, |s| Val::from(s.as_ref()))
}

/// Lua `sortedKeys`: numbers first (by value), then strings.
fn sorted_keys(t: &Table) -> Vec<Key> {
    let mut keys: Vec<Key> = t.pairs().map(|(k, _)| k).collect();
    keys.sort_by(|a, b| match (a, b) {
        (Key::I(x), Key::I(y)) => x.cmp(y),
        (Key::I(_), _) => std::cmp::Ordering::Less,
        (_, Key::I(_)) => std::cmp::Ordering::Greater,
        (Key::S(x), Key::S(y)) => x.as_bytes().cmp(y.as_bytes()),
        _ => std::cmp::Ordering::Equal,
    });
    keys
}

fn key_str(k: &Key) -> String {
    match k {
        Key::S(s) => s.to_string(),
        Key::I(i) => tostring_num(*i as f64),
        Key::B(b) => b.to_string(),
        Key::F(f) => tostring_num(f.get()),
    }
}

fn num_list(mut v: Vec<f64>) -> String {
    v.sort_by(|a, b| a.partial_cmp(b).unwrap());
    v.iter()
        .map(|x| lfmt!("%.17g", *x).to_string())
        .collect::<Vec<_>>()
        .join(",")
}

fn concat_nums<I: IntoIterator<Item = f64>>(it: I) -> String {
    it.into_iter()
        .map(tostring_num)
        .collect::<Vec<_>>()
        .join(",")
}

fn rust_dump(build: &BuildRef) -> Lines {
    let mut d = Dump::default();
    d.emit("build.targetVersion", build.targetVersion.get().as_str());
    d.emit("build.viewMode", build.viewMode.get().as_str());
    d.emit("build.characterLevel", build.characterLevel.get());
    d.emit(
        "build.characterLevelAutoMode",
        characterLevelAutoMode(build),
    );
    d.emit("build.bandit", build.bandit.get().as_str());
    d.emit(
        "build.pantheonMajorGod",
        build.pantheonMajorGod.get().as_str(),
    );
    d.emit(
        "build.pantheonMinorGod",
        build.pantheonMinorGod.get().as_str(),
    );
    d.emit("build.mainSocketGroup", build.mainSocketGroup.get());
    d.emit(
        "build.spectreList",
        build
            .spectreList
            .get()
            .snapshot()
            .iter()
            .map(|n| n.to_string())
            .collect::<Vec<_>>()
            .join(","),
    );
    let td = timelessData(build);
    let sub = |k: &str, f: &str| td.table(k).map_or(Val::Nil, |t| t.get(f));
    d.emit("build.timeless.jewelType", sub("jewelType", "id"));
    d.emit("build.timeless.conquerorType", sub("conquerorType", "id"));
    d.emit(
        "build.timeless.devotionVariant1",
        td.get("devotionVariant1"),
    );
    d.emit(
        "build.timeless.devotionVariant2",
        td.get("devotionVariant2"),
    );
    d.emit("build.timeless.jewelSocket", sub("jewelSocket", "id"));
    d.emit(
        "build.timeless.fallbackWeightMode",
        sub("fallbackWeightMode", "idx"),
    );
    for k in [
        "socketFilter",
        "socketAllocate",
        "socketFilterDistance",
        "searchList",
        "searchListFallback",
    ] {
        d.emit(format!("build.timeless.{k}"), td.get(k));
    }

    // items tab
    let it = &build.itemsTab;
    let order = it.orderList.snapshot();
    d.emit(
        "items.orderList",
        concat_nums(order.iter().map(|x| f64::from(*x))),
    );
    for id in &order {
        let item = it.items.with(|m| m.get(id).cloned()).expect("ordered item");
        let p = format!("items.item.{id}");
        d.emit(format!("{p}.name"), &*item.name.get());
        d.emit(format!("{p}.baseName"), opt_str(item.baseName.get()));
        d.emit(format!("{p}.rarity"), item.rarity.get().as_str());
        d.emit(format!("{p}.type"), item.r#type.get().as_str());
        d.emit(format!("{p}.raw"), &*item.raw.get());
        for (k, v) in [
            ("variant", item.variant.get()),
            ("variantAlt", item.variantAlt.get()),
            ("variantAlt2", item.variantAlt2.get()),
            ("variantAlt3", item.variantAlt3.get()),
            ("variantAlt4", item.variantAlt4.get()),
            ("variantAlt5", item.variantAlt5.get()),
        ] {
            d.emit(format!("{p}.{k}"), opt_num(v));
        }
        for k in [
            "hasAltVariant",
            "hasAltVariant2",
            "hasAltVariant3",
            "hasAltVariant4",
            "hasAltVariant5",
        ] {
            d.emit(format!("{p}.{k}"), item.extra.get(k));
        }
        let mut ranges = Vec::new();
        for list in [
            item.buffModLines.get(),
            item.enchantModLines.get(),
            item.scourgeModLines.get(),
            item.implicitModLines.get(),
            item.explicitModLines.get(),
            item.crucibleModLines.get(),
        ] {
            for l in list.snapshot() {
                ranges.push(match l.get("range") {
                    Val::Num(r) => lfmt!("%.17g", r).to_string(),
                    _ => "-".to_owned(),
                });
            }
        }
        d.emit(format!("{p}.ranges"), ranges.join(","));
    }
    d.emit("items.activeItemSetId", it.activeItemSetId.get());
    d.emit("items.showStatDifferences", it.showStatDifferences(build));
    d.emit(
        "items.itemSetOrderList",
        concat_nums(it.itemSetOrderList.snapshot().iter().map(|x| f64::from(*x))),
    );
    let mut setIds: Vec<u32> = it.itemSets.with(|m| m.keys().copied().collect());
    setIds.sort_unstable();
    for id in setIds {
        let set = it.itemSets.with(|m| m.get(&id).cloned()).unwrap();
        let p = format!("items.set.{id}");
        d.emit(format!("{p}.id"), set.get("id"));
        d.emit(format!("{p}.title"), set.get("title"));
        d.emit(
            format!("{p}.useSecondWeaponSet"),
            set.get("useSecondWeaponSet"),
        );
        for k in sorted_keys(&set) {
            if let Some(v) = set.table(k) {
                let ks = key_str(&k);
                d.emit(format!("{p}.slot.{ks}.selItemId"), v.get("selItemId"));
                d.emit(format!("{p}.slot.{ks}.active"), v.get("active"));
                d.emit(format!("{p}.slot.{ks}.pbURL"), v.get("pbURL"));
            }
        }
    }
    let active = it.activeItemSet.get();
    let byId = pob_calc::model::items_tab::id_key(it.activeItemSetId.get())
        .and_then(|k| it.itemSets.with(|m| m.get(&k).cloned()));
    d.emit(
        "items.activeSet",
        match (active, byId) {
            (Some(a), Some(b)) => a.ptr_eq(&b),
            (None, None) => true,
            _ => false,
        },
    );
    let ordered: Vec<String> = it
        .orderedSlots
        .snapshot()
        .iter()
        .map(|s| s.str("slotName").unwrap().to_string())
        .collect();
    d.emit("items.orderedSlots", ordered.join("|"));
    let mut names: Vec<Name> = it.slots.with(|m| m.keys().copied().collect());
    names.sort_by(|a, b| a.as_bytes().cmp(b.as_bytes()));
    for name in names {
        let slot = it.slots.with(|m| m.get(&name).cloned()).unwrap();
        let p = format!("items.slot.{name}");
        for k in [
            "selItemId",
            "active",
            "inactive",
            "weaponSet",
            "nodeId",
            "slotNum",
            "label",
        ] {
            d.emit(format!("{p}.{k}"), slot.get(k));
        }
        d.emit(
            format!("{p}.parentSlot"),
            slot.table("parentSlot")
                .map_or(Val::Nil, |t| t.get("slotName")),
        );
        let items = slot.table("items").unwrap();
        let mut ids: Vec<f64> = items.ipairs().iter().map(|(_, v)| v.num()).collect();
        ids.sort_by(|a, b| a.partial_cmp(b).unwrap());
        d.emit(format!("{p}.items"), concat_nums(ids));
        if let Some(a) = slot.table("controls").and_then(|c| c.table("activate")) {
            d.emit(format!("{p}.activateState"), a.get("state"));
        }
    }
    let sockets: Vec<f64> = it
        .sockets
        .with(|m| m.keys())
        .iter()
        .map(|k| *k as f64)
        .collect();
    d.emit("items.sockets", num_list(sockets));
    let extra = pob_calc::model::build_xml::build_extra(build).get_or_new_table("itemsTab");
    d.emit(
        "items.lastSlot",
        extra
            .table("lastSlot")
            .map_or(Val::Nil, |t| t.get("slotName")),
    );
    if let Some(ssl) = it.statSortSelectionList(build) {
        d.emit("items.statSort.n", ssl.len() as f64);
        for (i, s) in ssl.ipairs() {
            let s = s.table().unwrap();
            for k in ["label", "stat", "weightMult"] {
                d.emit(format!("items.statSort.{i}.{k}"), s.get(k));
            }
        }
    }

    // tree tab
    let tt = &build.treeTab;
    d.emit("tree.activeSpec", tt.activeSpec.get());
    let specs = tt.specList.snapshot();
    d.emit("tree.n", specs.len() as f64);
    let a = tt.activeSpec.get();
    d.emit(
        "tree.buildSpecIsActive",
        a >= 1.0
            && specs
                .get(a as usize - 1)
                .is_some_and(|s| Rc::ptr_eq(s, &build.spec.get())),
    );
    for (i, spec) in specs.iter().enumerate() {
        let p = format!("tree.spec.{}", i + 1);
        d.emit(format!("{p}.treeVersion"), spec.treeVersion.get().as_str());
        d.emit(format!("{p}.title"), opt_str(spec.title.get()));
        d.emit(format!("{p}.curClassId"), nan_nil(spec.curClassId.get()));
        d.emit(
            format!("{p}.curAscendClassId"),
            nan_nil(spec.curAscendClassId.get()),
        );
        d.emit(
            format!("{p}.curSecondaryAscendClassId"),
            nan_nil(spec.curSecondaryAscendClassId.get()),
        );
        let alloc: Vec<f64> = spec
            .allocNodes
            .with(|m| m.keys())
            .iter()
            .map(|k| *k as f64)
            .collect();
        d.emit(format!("{p}.alloc"), num_list(alloc));
        let mut j = spec.jewels.with(|m| m.pairs());
        j.sort_by_key(|(k, _)| *k);
        d.emit(
            format!("{p}.jewels"),
            j.iter()
                .map(|(k, v)| format!("{}={}", tostring_num(*k as f64), tostring_num(*v)))
                .collect::<Vec<_>>()
                .join(","),
        );
        let mut m = spec.masterySelections.with(|m| m.pairs());
        m.sort_by_key(|(k, _)| *k);
        d.emit(
            format!("{p}.mastery"),
            m.iter()
                .map(|(k, v)| format!("{}={}", tostring_num(*k as f64), tostring_num(*v)))
                .collect::<Vec<_>>()
                .join(","),
        );
    }

    // calcs tab
    let ct = &build.calcsTab;
    for k in sorted_keys(&ct.input) {
        d.emit(format!("calcs.input.{}", key_str(&k)), ct.input.get(k));
    }
    for (_, section) in ct.sectionList().ipairs() {
        let section = section.table().unwrap();
        let id = section.str("id").unwrap();
        for (_, sub) in section.table("subSection").unwrap().ipairs() {
            let sub = sub.table().unwrap();
            d.emit(
                format!("calcs.section.{id}.{}", sub.str("id").unwrap()),
                sub.get("collapsed"),
            );
        }
    }

    // skills / config skeleton
    let st = &build.skillsTab;
    d.emit("skills.activeSkillSetId", st.activeSkillSetId.get());
    d.emit(
        "skills.skillSetOrderList",
        concat_nums(
            st.skillSetOrderList
                .snapshot()
                .iter()
                .map(|x| f64::from(*x)),
        ),
    );
    d.emit("skills.groups", st.socketGroupList.get().len() as f64);
    d.emit("skills.defaultGemLevel", opt_str(st.defaultGemLevel.get()));
    d.emit(
        "skills.defaultGemQuality",
        opt_num(st.defaultGemQuality.get()),
    );
    let cfg = &build.configTab;
    d.emit("config.activeConfigSetId", cfg.activeConfigSetId.get());
    d.emit(
        "config.configSetOrderList",
        concat_nums(
            cfg.configSetOrderList
                .snapshot()
                .iter()
                .map(|x| f64::from(*x)),
        ),
    );
    for k in sorted_keys(&cfg.input) {
        let v = cfg.input.get(k);
        if !matches!(v, Val::Table(_) | Val::Data(_)) {
            d.emit(format!("config.input.{}", key_str(&k)), v);
        }
    }
    d.lines
}

fn parse_lines(s: &str) -> Lines {
    s.lines()
        .filter_map(|l| l.split_once('\t'))
        .map(|(k, v)| (k.to_owned(), v.to_owned()))
        .collect()
}

fn diff_lines(tag: &str, a: &Lines, b: &Lines, out: &mut Vec<String>) {
    for (k, va) in a {
        match b.get(k) {
            Some(vb) if vb == va => {}
            Some(vb) => out.push(format!("{tag} {k}: {} vs {}", short(va), short(vb))),
            None => out.push(format!("{tag} {k}: {} vs <nil>", short(va))),
        }
    }
    for (k, vb) in b {
        if !a.contains_key(k) {
            out.push(format!("{tag} {k}: <nil> vs {}", short(vb)));
        }
    }
}

fn short(s: &str) -> String {
    if s.len() > 160 {
        format!(
            "{}…",
            &s[..s.char_indices().nth(160).map_or(s.len(), |(i, _)| i)]
        )
    } else {
        s.to_owned()
    }
}

// ---- canonical XML (multiset comparison) ----------------------------------------------------

const SKIPPED_ROOT: [&str; 3] = ["Notes", "TreeView", "Import"];
const SKIPPED_BUILD: [&str; 3] = ["PlayerStat", "FullDPSSkill", "MinionStat"];

/// `order_free`: also sort the spec's `nodes` / `masteryEffects` lists and drop the tree URL
/// text, whose order follows `pairs(allocNodes)` — an `LjIntMap` whose order depends on the
/// insertion history, so it changes between a save and the save of the reloaded build (in PoB
/// as well).
fn canon(node: &XmlNode, depth: usize, order_free: bool) -> String {
    let mut attrs: Vec<String> = node
        .attrib
        .iter()
        .filter(|(k, _)| !(vaal_gem(node) && matches!(&***k, "nameSpec" | "skillId" | "variantId")))
        .map(|(k, v)| {
            if order_free && &*node.elem == "Spec" && (&**k == "nodes" || &**k == "masteryEffects")
            {
                let sep = if &**k == "nodes" { "," } else { "},{" };
                let mut l: Vec<&str> = v
                    .trim_matches(|c| c == '{' || c == '}')
                    .split(sep)
                    .collect();
                l.sort_unstable();
                format!("{k}={l:?}")
            } else {
                format!("{k}={v:?}")
            }
        })
        .collect();
    attrs.sort();
    let mut kids: Vec<String> = Vec::new();
    for c in &node.children {
        match c {
            XmlChild::Text(t) => {
                if !(order_free && &*node.elem == "URL") {
                    kids.push(format!("text:{t:?}"));
                }
            }
            XmlChild::Node(n) => {
                if depth == 0 && SKIPPED_ROOT.contains(&&*n.elem) {
                    continue;
                }
                if &*node.elem == "Build" && SKIPPED_BUILD.contains(&&*n.elem) {
                    continue;
                }
                kids.push(canon(n, depth + 1, order_free));
            }
        }
    }
    kids.sort();
    format!("<{} {}>[{}]", node.elem, attrs.join(" "), kids.join(","))
}

/// A Vaal skill gem: PoB resolves an old-format Vaal gem to the gem or to one of its
/// synthesised `AltX`/`AltY` variants depending on its per-process `pairs` order (CCR-0010
/// §9.5, CCR-0019 §11), so the name/skill/variant of such a gem are not compared.
fn vaal_gem(node: &XmlNode) -> bool {
    &*node.elem == "Gem"
        && node
            .attrib
            .get("gemId")
            .is_some_and(|g| g.starts_with("Metadata/Items/Gems/SkillGemVaal"))
}

/// The top-level sections of a saved XML as canonical strings by element name.
fn canon_sections(text: &str, order_free: bool) -> Result<BTreeMap<String, String>, String> {
    let (nodes, err) = ParseXML(text);
    if let Some(e) = err {
        return Err(e);
    }
    let root = nodes.into_iter().next().ok_or("no root")?;
    let mut out = BTreeMap::new();
    for c in &root.children {
        if let XmlChild::Node(n) = c {
            if SKIPPED_ROOT.contains(&&*n.elem) {
                continue;
            }
            out.insert(n.elem.to_string(), canon(n, 1, order_free));
        }
    }
    Ok(out)
}

/// The first place two canonical strings differ (for the report).
fn first_diff(a: &str, b: &str) -> String {
    let i = a
        .bytes()
        .zip(b.bytes())
        .position(|(x, y)| x != y)
        .unwrap_or(a.len().min(b.len()));
    let lo = i.saturating_sub(120);
    let cut = |s: &str| {
        let s = s.get(lo..).unwrap_or("");
        short(s)
    };
    format!("rust …{}\n          pob  …{}", cut(a), cut(b))
}

// ---- PoB -------------------------------------------------------------------------------------

struct PobRun {
    /// The model at the first BuildOutput of the load (bma.lua hook).
    pre: Lines,
    /// PoB's `SaveDB` at that point.
    preSave: String,
    /// The model after the oracle's load script (BuildOutput + `UpdateSockets`).
    post: Lines,
    outputs: OracleOutput,
}

/// PoB's side of one load, from the shared oracle query cache (TP1: `testkit::oracle_query`,
/// keyed by the PoB tree, the XML, the build name, the scripts and [`POB_RUN_CALLS`]) or a fresh
/// LuaJIT PoB.
fn pob_run(src: &Path, xml: &str, name: &str) -> Result<PobRun, String> {
    let text = pob_oracle::testkit::oracle_query(
        src,
        xml.as_bytes(),
        &[
            "BM-A pob_run 1",
            name,
            QUIET_PRELUDE,
            ORACLE_LUA,
            BMA_LUA,
            &POB_RUN_CALLS.join("\n"),
        ],
        || pob_run_texts(src, xml, name).map(|t| serde_json::Value::from(t.to_vec()).to_string()),
    )?;
    let t: Vec<String> = serde_json::from_str(&text).map_err(|e| format!("cached pob_run: {e}"))?;
    let [pre, preSave, post, outputs] =
        <[String; 4]>::try_from(t).map_err(|t| format!("cached pob_run: {} parts", t.len()))?;
    Ok(PobRun {
        pre: parse_lines(&pre),
        preSave,
        post: parse_lines(&post),
        outputs: OracleOutput::from_json(&outputs).map_err(|e| e.to_string())?,
    })
}

/// The Lua [`pob_run_texts`] runs after the load (pre dump, pre save, post dump, outputs), in
/// order; part of the query-cache key, so editing one invalidates the cached runs.
const POB_RUN_CALLS: [&str; 4] = [
    "return __bma.pre or 'error\\tno capture'",
    "return __bma.preSave or 'error\\tno capture'",
    "return __bma.dump()",
    "return __oracle_outputs()",
];

/// [`pob_run`]'s texts from a fresh PoB (the caller holds the oracle lock): the pre-BuildOutput
/// model dump, its `SaveDB`, the post-load model dump and the outputs JSON.
fn pob_run_texts(src: &Path, xml: &str, name: &str) -> Result<[String; 4], String> {
    let mut pob = JitPob::boot_oracle(src, Some(QUIET_PRELUDE))?;
    pob.run(ORACLE_LUA)?;
    pob.run(BMA_LUA)?;
    use pob_oracle::headless::Engine;
    pob.load_build_xml(xml, name)?;
    let text = |pob: &mut JitPob, code: &str| -> Result<String, String> {
        let r = pob.run(code)?;
        Ok(r.first().map(|r| r.text().to_owned()).unwrap_or_default())
    };
    let pre = text(&mut pob, POB_RUN_CALLS[0])?;
    if let Some(e) = pre.strip_prefix("error\t") {
        return Err(format!("pre dump: {e}"));
    }
    let preSave = text(&mut pob, POB_RUN_CALLS[1])?;
    if let Some(e) = preSave.strip_prefix("error\t") {
        return Err(format!("pre save: {e}"));
    }
    let post = text(&mut pob, POB_RUN_CALLS[2])?;
    let outputs = text(&mut pob, POB_RUN_CALLS[3])?;
    // a dump that does not parse is an error of this run (not cached)
    OracleOutput::from_json(&outputs).map_err(|e| e.to_string())?;
    Ok([pre, preSave, post, outputs])
}

fn leaf_eq(a: &pob_oracle::oracle::Leaf, b: &pob_oracle::oracle::Leaf) -> bool {
    use pob_oracle::oracle::Leaf;
    match (a, b) {
        (Leaf::Num(x), Leaf::Num(y)) => x == y || (x.is_nan() && y.is_nan()),
        _ => a == b,
    }
}

// ---- the test --------------------------------------------------------------------------------

#[derive(Default)]
struct Tally {
    builds: usize,
    loaded: usize,
    model_lines: usize,
    model_mismatch: usize,
    socket_lines: usize,
    socket_mismatch: usize,
    sections: usize,
    section_mismatch: usize,
    roundtrip_mismatch: usize,
    output_keys: usize,
    output_mismatch: usize,
    output_builds_equal: usize,
    pending_psb_builds: usize,
    pending_psb: usize,
    failures: Vec<String>,
}

impl Tally {
    /// Add one build's tally (TP1: builds run on the bounded pool, merged in corpus order).
    fn merge(&mut self, o: Tally) {
        self.builds += o.builds;
        self.loaded += o.loaded;
        self.model_lines += o.model_lines;
        self.model_mismatch += o.model_mismatch;
        self.socket_lines += o.socket_lines;
        self.socket_mismatch += o.socket_mismatch;
        self.sections += o.sections;
        self.section_mismatch += o.section_mismatch;
        self.roundtrip_mismatch += o.roundtrip_mismatch;
        self.output_keys += o.output_keys;
        self.output_mismatch += o.output_mismatch;
        self.output_builds_equal += o.output_builds_equal;
        self.pending_psb_builds += o.pending_psb_builds;
        self.pending_psb += o.pending_psb;
        self.failures.extend(o.failures);
    }
}

/// Hits of `site` recorded so far in this process (`unported!` counters are process-wide).
fn stub_hits(site: &str) -> u64 {
    pob_calc::stub::stub_report()
        .iter()
        .find(|(s, _)| *s == site)
        .map_or(0, |(_, n)| *n)
}

/// The `unported!` site of PS-B's `BuildAllDependsAndPaths`.
const PSB_SITE: &str = "Classes/PassiveSpec.lua:1092-1695";

/// The node ids of a dumped `tree.spec.N.alloc` value (`s:1,2,3`).
fn id_set(v: &str) -> std::collections::BTreeSet<&str> {
    v.split_once(':')
        .map_or(v, |(_, r)| r)
        .split(',')
        .filter(|x| !x.is_empty())
        .collect()
}

/// Model differences pending PS-B: a `tree.spec.N.alloc` whose Rust set strictly contains
/// PoB's. Removes them from `out` and returns how many.
fn split_psb_alloc(tag: &str, rust: &Lines, pob: &Lines, out: &mut Vec<String>) -> usize {
    let pending: Vec<String> = rust
        .iter()
        .filter(|(k, _)| k.starts_with("tree.spec.") && k.ends_with(".alloc"))
        .filter_map(|(k, rv)| {
            let pv = pob.get(k)?;
            let (r, p) = (id_set(rv), id_set(pv));
            (r != p && r.is_superset(&p)).then(|| format!("{tag} {k}: "))
        })
        .collect();
    let before = out.len();
    out.retain(|f| !pending.iter().any(|pfx| f.starts_with(pfx.as_str())));
    before - out.len()
}

/// A `<Tree>` section without the Spec `nodes` attributes and URL texts, and those node lists.
fn tree_without_nodes(text: &str) -> Option<(String, Vec<std::collections::BTreeSet<String>>)> {
    let (nodes, _) = ParseXML(text);
    let root = nodes.into_iter().next()?;
    let mut tree = root.children.into_iter().find_map(|c| match c {
        XmlChild::Node(n) if &*n.elem == "Tree" => Some(n),
        _ => None,
    })?;
    let mut lists = Vec::new();
    for c in &mut tree.children {
        let XmlChild::Node(spec) = c else { continue };
        if &*spec.elem != "Spec" {
            continue;
        }
        let ids = spec.attrib.shift_remove("nodes").unwrap_or_default();
        lists.push(id_set(&ids).into_iter().map(str::to_owned).collect());
        for k in &mut spec.children {
            if let XmlChild::Node(u) = k {
                if &*u.elem == "URL" {
                    u.children.clear();
                }
            }
        }
    }
    Some((canon(&tree, 1, true), lists))
}

/// A `<Tree>` difference pending PS-B: equal without the node lists, and each Rust spec's
/// node list a superset of PoB's.
fn tree_pending_psb(rust: &str, pob: &str) -> bool {
    match (tree_without_nodes(rust), tree_without_nodes(pob)) {
        (Some((rc, rl)), Some((pc, pl))) => {
            rc == pc && rl.len() == pl.len() && rl.iter().zip(&pl).all(|(r, p)| r.is_superset(p))
        }
        _ => false,
    }
}

const SOCKET_KEYS: [&str; 2] = [".inactive", ".label"];

/// Compares two saved XML texts section by section (canonical, multiset). `psb`: PS-B's
/// `BuildAllDependsAndPaths` is a stub for this build (see the module doc).
fn compare_saves(
    tag: &str,
    rust: &str,
    pob: &str,
    psb: bool,
    t: &mut Tally,
    fails: &mut Vec<String>,
) {
    match (canon_sections(rust, false), canon_sections(pob, false)) {
        (Ok(r), Ok(p)) => {
            for (k, pv) in &p {
                t.sections += 1;
                match r.get(k) {
                    Some(rv) if rv == pv => {}
                    Some(_) if psb && k == "Tree" && tree_pending_psb(rust, pob) => {
                        t.pending_psb += 1;
                    }
                    Some(rv) => {
                        t.section_mismatch += 1;
                        fails.push(format!(
                            "{tag} <{k}> differs:\n          {}",
                            first_diff(rv, pv)
                        ));
                    }
                    None => {
                        t.section_mismatch += 1;
                        fails.push(format!("{tag} <{k}> missing in the Rust XML"));
                    }
                }
            }
            for k in r.keys() {
                if !p.contains_key(k) {
                    t.section_mismatch += 1;
                    fails.push(format!("{tag} <{k}> only in the Rust XML"));
                }
            }
        }
        (r, p) => fails.push(format!(
            "{tag}: parse error rust={:?} pob={:?}",
            r.err(),
            p.err()
        )),
    }
}

/// One build: its tally and its report lines (printed by the caller in corpus order).
fn run_build(
    src: &Path,
    data: &'static Data,
    path: &Path,
    saveDir: Option<&Path>,
) -> (Tally, String) {
    let mut t = Tally::default();
    let mut log = String::new();
    // each build from a clean (thread-local) GlobalCache, as PoB's fresh process per build: under
    // the pool, which builds came before on this worker is scheduling, not corpus order
    pob_calc::global_cache::clear_all();
    run_build_into(src, data, path, &mut t, &mut log, saveDir);
    (t, log)
}

fn run_build_into(
    src: &Path,
    data: &'static Data,
    path: &Path,
    t: &mut Tally,
    log: &mut String,
    saveDir: Option<&Path>,
) {
    use std::fmt::Write as _;
    let stem = path.file_stem().unwrap().to_string_lossy().into_owned();
    let xml = std::fs::read_to_string(path).unwrap();
    t.builds += 1;
    let pending_before = t.pending_psb;
    let mut fails: Vec<String> = Vec::new();

    // Rust: load, dump, save; UpdateSockets; round trip. TP1: the `unported!` counters are
    // process-wide and other builds load concurrently, so the PSB stub is detected by the growth
    // of its counter over this load (no reset), and a hit under `POB_TEST_PAR > 1` fails the run
    // instead of being attributed (it could belong to a concurrent build).
    let psb_before = stub_hits(PSB_SITE);
    let rust = catch(|| {
        let build = loadBuildModelFromXML(&xml, data).map_err(|e| format!("load: {e}"))?;
        let d1 = rust_dump(&build);
        let saved = build.SaveDB("bma").ok_or("SaveDB returned nil")?;
        build.itemsTab.UpdateSockets(&build);
        let d1u = rust_dump(&build);
        drop(build);
        let b2 = loadBuildModelFromXML(&saved, data).map_err(|e| format!("reload: {e}"))?;
        let d2 = rust_dump(&b2);
        let saved2 = b2.SaveDB("bma").ok_or("SaveDB returned nil")?;
        Ok::<_, String>((d1, saved, d1u, d2, saved2))
    });
    let (d1, saved, d1u, d2, saved2) = match rust {
        Ok(Ok(x)) => x,
        Ok(Err(e)) | Err(e) => {
            t.failures.push(format!("{stem}: Rust {e}"));
            return;
        }
    };
    t.loaded += 1;
    let psb = stub_hits(PSB_SITE) > psb_before;
    if psb && pob_oracle::testkit::test_par() > 1 {
        t.failures.push(format!(
            "{stem}: the PS-B stub {PSB_SITE} was hit while builds load in parallel: rerun with POB_TEST_PAR=1 to attribute its differences"
        ));
        return;
    }
    // PoB on the original
    let a = match pob_run(src, &xml, &stem) {
        Ok(a) => a,
        Err(e) => {
            t.failures.push(format!("{stem}: PoB (original): {e}"));
            return;
        }
    };
    let mut m = Vec::new();
    diff_lines("load", &d1, &a.pre, &mut m);
    if psb {
        t.pending_psb += split_psb_alloc("load", &d1, &a.pre, &mut m);
    }
    t.model_lines += a.pre.len();
    t.model_mismatch += m.len();
    fails.extend(m);
    // UpdateSockets (the oracle's load script runs it after the load)
    let pick = |l: &Lines| -> Lines {
        l.iter()
            .filter(|(k, _)| {
                (k.starts_with("items.slot.Jewel ") && SOCKET_KEYS.iter().any(|s| k.ends_with(s)))
                    || *k == "items.lastSlot"
            })
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect()
    };
    let (su, sp) = (pick(&d1u), pick(&a.post));
    let mut s = Vec::new();
    diff_lines("sockets", &su, &sp, &mut s);
    t.socket_lines += sp.len();
    t.socket_mismatch += s.len();
    fails.extend(s);

    // Rust SaveDB vs PoB SaveDB (both before the first BuildOutput)
    compare_saves("save", &saved, &a.preSave, psb, t, &mut fails);
    if let Some(dir) = saveDir {
        let _ = std::fs::create_dir_all(dir);
        let _ = std::fs::write(dir.join(format!("{stem}.rust.xml")), &saved);
        let _ = std::fs::write(dir.join(format!("{stem}.rust2.xml")), &saved2);
        let _ = std::fs::write(dir.join(format!("{stem}.pob.xml")), &a.preSave);
    }

    // PoB on the Rust-saved XML
    match pob_run(src, &saved, &stem) {
        Ok(b) => {
            // the round trip: Rust's model of the Rust-saved XML is PoB's model of it
            let mut r = Vec::new();
            diff_lines("roundtrip", &d2, &b.pre, &mut r);
            if psb {
                t.pending_psb += split_psb_alloc("roundtrip", &d2, &b.pre, &mut r);
            }
            t.roundtrip_mismatch += r.len();
            compare_saves("roundtrip save", &saved2, &b.preSave, psb, t, &mut r);
            fails.extend(r);
            let fa = a.outputs.flat();
            let fb = b.outputs.flat();
            t.output_keys += fa.len();
            let mut n = 0;
            for (k, va) in &fa {
                match fb.get(k) {
                    Some(vb) if leaf_eq(va, vb) => {}
                    other => {
                        n += 1;
                        if n <= 10 {
                            fails.push(format!("outputs {k}: {va:?} vs {other:?}"));
                        }
                    }
                }
            }
            for k in fb.keys() {
                if !fa.contains_key(k) {
                    n += 1;
                    if n <= 10 {
                        fails.push(format!("outputs {k}: only with the Rust-saved XML"));
                    }
                }
            }
            if n == 0 {
                t.output_builds_equal += 1;
            }
            t.output_mismatch += n;
        }
        Err(e) => fails.push(format!("PoB (Rust-saved): {e}")),
    }

    if t.pending_psb > pending_before {
        t.pending_psb_builds += 1;
    }
    if fails.is_empty() {
        let _ = writeln!(log, "ok   {stem}");
    } else {
        let _ = writeln!(log, "FAIL {stem}: {} differences", fails.len());
        for f in fails.iter().take(25) {
            let _ = writeln!(log, "       {f}");
        }
        t.failures
            .push(format!("{stem}: {} differences", fails.len()));
    }
}

#[test]
fn bm_a_corpus() {
    let _serial = SERIAL
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let Some(src) = find_pob_src() else {
        return skip("PoB checkout (.PathOfBuilding/src) not found");
    };
    let Some(data) = init_data() else {
        return skip("data/dump/data.bin not found");
    };
    let Some(parity) = find_parity_dir() else {
        return skip(".parity not found");
    };
    quiet_lua_errors();
    // Timeless jewel LUTs load from the PoB checkout (PS-B builds jewel
    // node data through them); the default `StdJewelFs::new("")` probes
    // the crate directory and finds nothing.
    let src = std::fs::canonicalize(&src).unwrap_or(src);
    lut::set_jewel_fs(Arc::new(lut::StdJewelFs {
        script_path: src.to_string_lossy().into_owned(),
        writable: false,
    }));
    lut::set_con_printf(|_| {});
    let mut builds: Vec<PathBuf> = std::fs::read_dir(parity.join("builds"))
        .map(|rd| rd.filter_map(|e| e.ok().map(|e| e.path())).collect())
        .unwrap_or_default();
    builds.retain(|p| p.extension().is_some_and(|e| e == "xml"));
    if let Ok(f) = std::env::var("BMA_BUILDS") {
        builds.retain(|p| p.to_string_lossy().contains(&f));
    }
    builds.sort();
    if builds.is_empty() {
        return skip(".parity/builds/*.xml not found");
    }
    let saveDir = std::env::var_os("BMA_SAVE_DIR").map(PathBuf::from);
    let mut t = Tally::default();
    // TP1: builds on the bounded pool (`$POB_TEST_PAR`); tallies and report lines are merged here
    // in corpus order, so the report is the serial one
    pob_oracle::testkit::par_for_each(
        &builds,
        |_, b| {
            let r = run_build(&src, data, b, saveDir.as_deref());
            // Drop the decoded LUTs between builds to keep RSS low: serial runs only (the LUT
            // table is process-global; a clear while another worker is inside `readLUT` makes
            // its entry vanish), else once after the pool.
            pob_oracle::testkit::release_shared(|| lut::clear_timeless_jewel_luts(None));
            r
        },
        |_, _, (bt, log)| {
            eprint!("{log}");
            t.merge(bt);
        },
    );
    lut::clear_timeless_jewel_luts(None);
    eprintln!("{}", pob_oracle::runner::query_cache_summary());
    eprintln!(
        "BM-A: {} builds, {} loaded in Rust; model {} lines, {} mismatches; sockets {} lines, \
         {} mismatches; save sections {} compared, {} differ; round trip {} mismatches; \
         outputs {} keys, {} differ ({} builds identical); pending PS-B \
         (BuildAllDependsAndPaths stub): {} differences on {} builds",
        t.builds,
        t.loaded,
        t.model_lines,
        t.model_mismatch,
        t.socket_lines,
        t.socket_mismatch,
        t.sections,
        t.section_mismatch,
        t.roundtrip_mismatch,
        t.output_keys,
        t.output_mismatch,
        t.output_builds_equal,
        t.pending_psb,
        t.pending_psb_builds
    );
    assert!(
        t.failures.is_empty(),
        "BM-A failures:\n{}",
        t.failures.join("\n")
    );
}

#[test]
fn xml_reader_writer() {
    use pob_calc::model::xml::{ComposeXML, ParseXML_root};
    // entities, CDATA, comments, processing instructions, empty elements, text runs
    let text = "<?xml version=\"1.0\"?>\n<!-- c --><Root a=\"1&amp;2\" b='x'>\n  hello &lt;w&gt; &bogus;\n  <E k=\"v\"/>\n  <![CDATA[ raw <x> ]]>\n  <F></F>\n</Root>\n";
    let root = ParseXML_root(text).unwrap();
    assert_eq!(root.children.len(), 1);
    let XmlChild::Node(r) = &root.children[0] else {
        panic!()
    };
    assert_eq!(&*r.elem, "Root");
    assert_eq!(r.attrib.get("a").map(|s| &**s), Some("1&2"));
    assert_eq!(r.attrib.get("b").map(|s| &**s), Some("x"));
    let kinds: Vec<String> = r
        .children
        .iter()
        .map(|c| match c {
            XmlChild::Text(t) => format!("T:{t}"),
            XmlChild::Node(n) => format!("N:{}", n.elem),
        })
        .collect();
    assert_eq!(kinds, ["T:hello <w> ", "N:E", "T: raw <x> ", "N:F"]);
    // errors as xml.lua words them
    assert_eq!(
        ParseXML_root("<A></B>").unwrap_err(),
        "trying to close <A> with 'B'"
    );
    assert_eq!(
        ParseXML_root("</B>").unwrap_err(),
        "nothing to close with 'B'"
    );
    assert_eq!(ParseXML_root("<A><B>").unwrap_err(), "unclosed element 'B'");
    // writer
    let s = ComposeXML(r);
    assert!(s.starts_with("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<Root a=\"1&amp;2\" b=\"x\">\n\thello &lt;w&gt; \n\t<E k=\"v\"/>\n"));
    assert!(s.ends_with("\t<F/>\n</Root>\n"));
}
