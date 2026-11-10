//! Injection of a pob-state/1 dump into the Rust calc graph (calc doc §9.2,
//! "Inject and export").
//!
//! Owner: ORC-X. [`state`] rebuilds the Rust graph of a probe hit (env,
//! actors, active skills, skill effects, cfgs, mod stores, items, gem
//! instances, socket groups, every dynamic table) with its sharing intact,
//! and returns the dumped locals by name, which
//! `pob_calc::probe::ProbeState::probe_set` maps onto a segment ctx.
//!
//! It works in three passes over the dump:
//!
//! 1. **Kinds.** Every table of the dump (identified by its `$id`, or by its
//!    position when it has none) gets the Rust kind its typed contexts ask
//!    for (`env.player` is an actor, `activeSkill.supportList` a list of skill
//!    effects, …; `pob_calc::probe::member_kind`), or a structural one (`$store`,
//!    `$item`, `$uuid`, the breakdown closures); every other table is a
//!    dynamic `Table`. Typed contexts win over dynamic ones, so a table that is
//!    both `activeSkill.weapon1Cfg` and `pass.cfg` is one `ModCfg`.
//! 2. **Allocation**, parents first, binding the immutable handles a Rust
//!    struct owns (`env.player`, `env.configInput`, `store.conditions`,
//!    `actor.itemList`, `item.requirements`) to the dumped tables.
//! 3. **Fill**: every member through `pob_calc::probe::set_member`, the same
//!    reflection the exporter reads, so `export(inject(dump)) == dump` is a
//!    test of both (tests/ORC-X.rs). What a typed field cannot hold falls back
//!    to the struct's `extra` table or to the probe registry (CCR-0030) and is
//!    counted in [`Injected::fallbacks`].
//!
//! Markers: `$data` paths resolve to the typed data (`GrantedEffectRef`,
//! `GemDataRef`, `SkillLevelRef`, `ItemBaseRef`, `MinionDataRef`) or stay a
//! `DataRef`; `$node`/`$treeNode` resolve against the build's spec / tree,
//! else to placeholder nodes (one per id); `$object` Build / PassiveSpec are
//! the supplied build and its spec; other envs' objects (`$foreign`, `$env`)
//! become placeholders that export as the same marker.

use std::collections::{BTreeMap, HashMap};
use std::rc::{Rc, Weak};
use std::sync::Arc;

use pob_calc::active_skill::{ActiveSkill, SkillEffect};
use pob_calc::actor::{Actor, ActorRef};
use pob_calc::breakdown::Breakdown;
use pob_calc::cfg::ModCfg;
use pob_calc::classes::{Item, ItemRef, ModStore, ModStoreRef, Node, NodeRef};
use pob_calc::env::{Env, EnvRef, Mode, Override};
use pob_calc::global_cache::{set_cachedData, CachedData};
use pob_calc::lua_table::{IndexMeta, Val};
use pob_calc::model::{BuildRef, GemInstance, SocketGroup};
use pob_calc::probe::{
    self as probe, element_kind, key_kind, member_kind, mark_absent, rc_ptr, set_member,
    typed_keys, with_registry, DataRef, DataTarget, Kind, Marker, PKey, PObj, ProbeField, PV,
    SBox, BREAKDOWN_FNS, ITEM_FIELDS,
};
use pob_calc::Table;
use pob_gamedata::{ModRef, Name, SkillTypeSet};
use pob_lua::{LjIntMap, PtrMap, PtrSet, Shared};
use serde_json::{Map, Value as J};

use crate::schema::StateDump;

/// The dumped locals of a probe hit, by Lua local name.
pub type Locals = BTreeMap<String, Val>;

/// Rebuild the Rust graph of a probe dump on top of `build` (which supplies the
/// build model the dump refers to: `env.build`, `env.spec`, config tables).
/// `None` when the dump has no env or cannot be injected (see [`inject`] for
/// the reason).
pub fn state(dump: &StateDump, build: &BuildRef) -> Option<(EnvRef, Locals)> {
    let i = inject(dump, build).ok()?;
    Some((i.env?, i.locals))
}

/// The result of an injection.
pub struct Injected {
    pub env: Option<EnvRef>,
    pub locals: Locals,
    /// "<kind>.<key>: reason" → count of values a typed field could not take.
    pub fallbacks: BTreeMap<String, u64>,
    /// Kind conflicts and unresolvable markers (diagnostics).
    pub notes: Vec<String>,
}

/// [`state`] with diagnostics. Resets the probe registry first (one injected
/// graph per thread at a time).
pub fn inject(dump: &StateDump, build: &BuildRef) -> Result<Injected, String> {
    probe::registry_reset();
    inject_keep(dump, build)
}

/// [`inject`] without the registry reset: the registry's side entries (object-keyed table
/// entries, absent-field marks, `$foreign` placeholders) of graphs injected earlier on this
/// thread are kept, so several documents can be injected one after the other (the caller
/// resets once, `pob_calc::probe::registry_reset`). The GlobalCache is still the last
/// document's. CCR-0088 §2.
pub fn inject_keep(dump: &StateDump, build: &BuildRef) -> Result<Injected, String> {
    pob_calc::global_cache::clear_all();
    run(dump, build, None)
}

/// Write a complete state onto a live graph: every dumped table found in the
/// live graph at one of its dump paths (`env.player.output`,
/// `env.player.mainSkill`, …; first the paths under `env`) is emptied and
/// refilled in place, keeping its identity; the others are created. The
/// GlobalCache is replaced by the dump's. Replay passes the live state merged
/// with a callee's `in → out` delta (`crate::replay::merge`), never a
/// recorded `:out` as is (that would overwrite the caller's own state).
pub fn apply_onto(dump: &StateDump, build: &BuildRef, env: &EnvRef) -> Result<Injected, String> {
    pob_calc::global_cache::clear_all();
    run(dump, build, Some(env))
}

fn run(dump: &StateDump, build: &BuildRef, live: Option<&EnvRef>) -> Result<Injected, String> {
    let root = &dump.root;
    let mut inj = Inj::new(build);
    inj.index(root);
    // pass 1: kinds
    if let Some(env) = root.get("env").filter(|e| !e.is_null()) {
        inj.visit(env, Kind::Env);
    }
    if let Some(J::Object(locals)) = root.get("locals") {
        for v in locals.values() {
            inj.visit(v, Kind::Any);
        }
    }
    if let Some(gc) = root.get("globalCache") {
        for entry in cache_entries(gc) {
            inj.visit(entry, Kind::Table);
            if let Some(s) = entry.get("ActiveSkill") {
                inj.visit(s, Kind::Skill);
            }
            if let Some(e) = entry.get("Env") {
                inj.visit(e, Kind::Env);
            }
        }
    }
    inj.run_worklist();
    // replay: bind the dumped tables to the live ones at the same paths
    if let Some(env) = live {
        inj.seed(root, env);
    }
    // pass 2: allocation
    inj.allocate()?;
    // pass 3: fill
    inj.fill_all();
    inj.patch_breakdowns();
    let env = match root.get("env").filter(|e| !e.is_null()) {
        Some(e) => inj.decode(e, Kind::Env).obj::<Env>(),
        None => None,
    };
    let mut locals = Locals::new();
    if let Some(J::Object(l)) = root.get("locals") {
        for (k, v) in l {
            let Some(name) = unescape(k) else { continue };
            let val = inj.decode(v, Kind::Any);
            locals.insert(name.to_owned(), inj.patched_breakdown(val));
        }
    }
    if let Some(gc) = root.get("globalCache") {
        inj.fill_global_cache(gc);
    }
    let fallbacks = with_registry(|r| r.fallbacks.clone().into_iter().collect());
    Ok(Injected {
        env,
        locals,
        fallbacks,
        notes: inj.notes,
    })
}

fn cache_entries(gc: &J) -> Vec<&J> {
    let mut out = Vec::new();
    if let Some(J::Object(modes)) = gc.get("cachedData") {
        for m in modes.values() {
            if let J::Object(entries) = m {
                for e in entries.values() {
                    out.push(e);
                }
            }
        }
    }
    out
}

/// `"$$x"` → `"$x"`; reserved `$` members → None.
fn unescape(k: &str) -> Option<&str> {
    match k.strip_prefix('$') {
        Some(rest) if rest.starts_with('$') => Some(rest),
        Some(_) => None,
        None => Some(k),
    }
}

/// Identity of a dumped table: its `$id`, or its address in the dump.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
enum NK {
    Id(u64),
    Addr(usize),
}

const MOD_TYPES: &[&str] = &[
    "BASE", "INC", "MORE", "FLAG", "OVERRIDE", "LIST", "MAX", "MIN", "Flag", "CHANCE", "DUMMY",
];

/// state_dump.lua `isMod`.
pub(crate) fn is_mod(m: &Map<String, J>) -> bool {
    m.get("name").is_some_and(J::is_string)
        && m
            .get("type")
            .and_then(J::as_str)
            .is_some_and(|t| MOD_TYPES.contains(&t))
        && !m.contains_key("$id")
        && !m.contains_key("$store")
        && !m.contains_key("$item")
}

/// A single-member `$` marker (never a table).
pub(crate) fn marker_key(m: &Map<String, J>) -> Option<&str> {
    if m.contains_key("$fn") {
        return Some("$fn");
    }
    if m.len() != 1 {
        return None;
    }
    let (k, _) = m.iter().next()?;
    if k.starts_with('$')
        && !k.starts_with("$$")
        && !matches!(k.as_str(), "$seq" | "$num" | "$keys" | "$meta" | "$id")
    {
        Some(k.as_str())
    } else {
        None
    }
}

struct Inj<'a> {
    build: BuildRef,
    defs: HashMap<u64, &'a J>,
    kinds: HashMap<NK, Kind>,
    nodes_def: HashMap<NK, &'a J>,
    work: Vec<NK>,
    objs: HashMap<NK, PObj>,
    tree_nodes: HashMap<(bool, i64), NodeRef>,
    breakdowns: HashMap<usize, Breakdown>,
    /// replay: dumped tables bound to live objects (emptied, then refilled)
    seeded: std::collections::HashSet<NK>,
    /// replay: identity keys of the seeded live objects
    seeded_keys: std::collections::HashSet<(u8, usize)>,
    notes: Vec<String>,
}

/// One step of a dump path.
#[derive(Clone, Debug)]
enum Step {
    S(String),
    N(f64),
    Meta,
}

impl<'a> Inj<'a> {
    fn new(build: &BuildRef) -> Self {
        Inj {
            build: build.clone(),
            defs: HashMap::new(),
            kinds: HashMap::new(),
            nodes_def: HashMap::new(),
            work: Vec::new(),
            objs: HashMap::new(),
            tree_nodes: HashMap::new(),
            breakdowns: HashMap::new(),
            seeded: std::collections::HashSet::new(),
            seeded_keys: std::collections::HashSet::new(),
            notes: Vec::new(),
        }
    }

    // ---- replay: binding dumped tables to live ones ----

    /// Every path (up to 8) at which each dumped table occurs.
    fn collect_paths(&self, root: &'a J) -> HashMap<NK, Vec<Vec<Step>>> {
        let mut out: HashMap<NK, Vec<Vec<Step>>> = HashMap::new();
        let mut stack: Vec<(&'a J, Vec<Step>)> = vec![(root, Vec::new())];
        while let Some((j, path)) = stack.pop() {
            let Some((nk, _)) = self.node_of(j) else {
                continue;
            };
            let e = out.entry(nk).or_default();
            if e.len() < 8 {
                e.push(path.clone());
            }
            let J::Object(m) = j else {
                if let J::Array(a) = j {
                    for (i, x) in a.iter().enumerate() {
                        let mut p = path.clone();
                        p.push(Step::N((i + 1) as f64));
                        stack.push((x, p));
                    }
                }
                continue;
            };
            if m.len() == 1 && m.contains_key("$ref") {
                continue;
            }
            let is_store = m.contains_key("$store");
            for (k, x) in m {
                match k.as_str() {
                    "$id" | "$store" | "$item" | "$uuid" | "$keys" => {}
                    "mods" if is_store => {}
                    "$seq" => {
                        if let J::Array(a) = x {
                            for (i, y) in a.iter().enumerate() {
                                let mut p = path.clone();
                                p.push(Step::N((i + 1) as f64));
                                stack.push((y, p));
                            }
                        }
                    }
                    "$num" => {
                        if let J::Object(n) = x {
                            for (nk, y) in n {
                                if let Ok(v) = nk.parse::<f64>() {
                                    let mut p = path.clone();
                                    p.push(Step::N(v));
                                    stack.push((y, p));
                                }
                            }
                        }
                    }
                    "$meta" => {
                        let mut p = path.clone();
                        p.push(Step::Meta);
                        stack.push((x, p));
                    }
                    _ => {
                        if let Some(name) = unescape(k) {
                            let mut p = path.clone();
                            p.push(Step::S(name.to_owned()));
                            stack.push((x, p));
                        }
                    }
                }
            }
        }
        out
    }

    fn navigate(root: &PObj, path: &[Step]) -> Option<PObj> {
        let mut cur = root.clone();
        for st in path {
            let pv = match st {
                Step::S(s) => probe::member_get(&cur, &PKey::S(s.clone())),
                Step::N(n) => probe::member_get(&cur, &PKey::N(*n)),
                Step::Meta => probe::meta_get(&cur),
            }?;
            cur = probe::pv_obj(&pv)?;
        }
        Some(cur)
    }

    /// Bind each dumped table to the live object at one of its paths.
    fn seed(&mut self, root: &'a J, env: &EnvRef) {
        let live_root = PObj::Synth(Rc::new(probe::Synth {
            members: vec![
                ("env".to_owned(), Val::Obj(env.clone())),
                ("globalCache".to_owned(), Val::Nil),
            ],
        }));
        let paths = self.collect_paths(root);
        let kinds: Vec<(NK, Kind)> = self.kinds.iter().map(|(k, v)| (*k, *v)).collect();
        for (nk, kind) in kinds {
            let Some(ps) = paths.get(&nk) else { continue };
            let mut ps: Vec<&Vec<Step>> = ps.iter().collect();
            ps.sort_by_key(|p| {
                let under_env = matches!(p.first(), Some(Step::S(s)) if s == "env");
                (!under_env, p.len())
            });
            for p in ps {
                if matches!(p.first(), Some(Step::S(s)) if s == "locals") {
                    continue;
                }
                let Some(live) = Self::navigate(&live_root, p) else {
                    continue;
                };
                let lk = probe::pobj_kind(&live);
                let fits = lk == kind
                    || (!kind.is_strong() && matches!(lk, Kind::Table | Kind::Breakdown))
                    || (kind == Kind::Breakdown && lk == Kind::Table);
                let synthetic = matches!(
                    live,
                    PObj::Synth(_) | PObj::GlobalCache | PObj::CacheData | PObj::CacheMode(_)
                );
                if fits && !synthetic {
                    self.seeded_keys.insert(live.key());
                    self.objs.insert(nk, live);
                    self.seeded.insert(nk);
                    break;
                }
            }
        }
    }

    fn note(&mut self, s: String) {
        if self.notes.len() < 200 {
            self.notes.push(s);
        }
    }

    // ---- indexing ----

    fn index(&mut self, v: &'a J) {
        let mut stack = vec![v];
        while let Some(v) = stack.pop() {
            match v {
                J::Array(a) => stack.extend(a.iter()),
                J::Object(m) => {
                    if marker_key(m).is_some() {
                        continue;
                    }
                    if let Some(id) = m.get("$id").and_then(J::as_u64) {
                        self.defs.insert(id, v);
                    }
                    for (k, x) in m {
                        if k == "mods" && m.contains_key("$store") {
                            continue;
                        }
                        stack.push(x);
                    }
                }
                _ => {}
            }
        }
    }

    /// The identity and full definition of a table value (None for scalars,
    /// markers and mods).
    fn node_of(&self, j: &'a J) -> Option<(NK, &'a J)> {
        match j {
            J::Array(_) => Some((NK::Addr(std::ptr::from_ref(j) as usize), j)),
            J::Object(m) => {
                if m.len() == 1 {
                    if let Some(id) = m.get("$ref").and_then(J::as_u64) {
                        return self.defs.get(&id).map(|d| (NK::Id(id), *d));
                    }
                }
                if marker_key(m).is_some() || is_mod(m) {
                    return None;
                }
                match m.get("$id").and_then(J::as_u64) {
                    Some(id) => Some((NK::Id(id), j)),
                    None => Some((NK::Addr(std::ptr::from_ref(j) as usize), j)),
                }
            }
            _ => None,
        }
    }

    // ---- pass 1: kinds ----

    fn structural(def: &J) -> Option<Kind> {
        let m = def.as_object()?;
        if m.contains_key("$store") {
            return Some(Kind::Store);
        }
        if m.contains_key("$item") {
            return Some(Kind::Item);
        }
        if m.contains_key("$uuid") {
            return Some(Kind::Skill);
        }
        let is_bd_fn = |k: &str, src: &str| {
            m.get(k)
                .and_then(|f| f.get("$fn"))
                .and_then(J::as_str)
                .is_some_and(|s| s == src)
        };
        if BREAKDOWN_FNS.iter().all(|(k, src)| is_bd_fn(k, src)) {
            return Some(Kind::Breakdown);
        }
        None
    }

    fn visit(&mut self, j: &'a J, expect: Kind) {
        let Some((nk, def)) = self.node_of(j) else {
            return;
        };
        let expect = if expect.is_leaf() { Kind::Any } else { expect };
        // a typed list whose elements do not fit (CCR-0030: `env.explodeSources`
        // holds nodes and items) stays a dynamic table
        let expect = if self.list_fits(expect, def) {
            expect
        } else {
            self.note(format!("{expect:?} with elements of another type: dynamic table"));
            Kind::Table
        };
        let old = self.kinds.get(&nk).copied();
        let st = Self::structural(def);
        let new = match (st, old) {
            (Some(s), _) => {
                if expect.is_strong() && expect != s && !(expect == Kind::Store && s == Kind::Store) {
                    let msg = format!("{nk:?}: {s:?} expected as {expect:?}");
                    self.note(msg);
                }
                s
            }
            (None, Some(o)) if o.is_strong() => {
                if expect.is_strong() && expect != o {
                    let msg = format!("{nk:?}: {o:?} also expected as {expect:?}");
                    self.note(msg);
                }
                o
            }
            (None, _) if expect.is_strong() => expect,
            (None, Some(o)) => {
                if expect == Kind::Table {
                    Kind::Table
                } else {
                    o
                }
            }
            (None, None) => expect,
        };
        if old != Some(new) {
            self.kinds.insert(nk, new);
            self.nodes_def.insert(nk, def);
            self.work.push(nk);
        }
    }

    /// Whether the elements of `def` fit the element type of list kind `k`.
    fn list_fits(&self, k: Kind, def: &'a J) -> bool {
        let elem_ok: fn(&Self, &'a J) -> bool = match k {
            Kind::SkillList | Kind::EffectList | Kind::GemList | Kind::TableList => {
                |s, j| match j {
                    J::Array(_) => true,
                    J::Object(m) => {
                        if let Some(mk) = marker_key(m) {
                            return match mk {
                                "$ref" => s.node_of(j).is_some_and(|(_, d)| {
                                    !d.as_object().is_some_and(|dm| {
                                        dm.contains_key("$item") || dm.contains_key("$store")
                                    })
                                }),
                                "$foreign" | "$env" => true,
                                _ => false,
                            };
                        }
                        !is_mod(m) && !m.contains_key("$item") && !m.contains_key("$store")
                    }
                    _ => false,
                }
            }
            Kind::NameList => |_, j| j.is_string(),
            Kind::ModVec => |_, j| j.as_object().is_some_and(is_mod),
            _ => return true,
        };
        Self::members(def).iter().all(|(k, x)| matches!(k, PKeyJ::N(_)) && elem_ok(self, x))
    }

    fn run_worklist(&mut self) {
        while let Some(nk) = self.work.pop() {
            let kind = self.kinds[&nk];
            let def = self.nodes_def[&nk];
            self.visit_children(kind, def);
        }
    }

    fn visit_children(&mut self, kind: Kind, def: &'a J) {
        match def {
            J::Array(a) => {
                let ek = element_kind(kind);
                for x in a {
                    self.visit(x, ek);
                }
            }
            J::Object(m) => {
                let ek = element_kind(kind);
                for (k, x) in m {
                    match k.as_str() {
                        "$id" | "$item" | "$store" | "$uuid" => {}
                        "mods" if kind == Kind::Store => {}
                        "$seq" => {
                            if let J::Array(a) = x {
                                for y in a {
                                    self.visit(y, ek);
                                }
                            }
                        }
                        "$num" => {
                            if let J::Object(n) = x {
                                for y in n.values() {
                                    self.visit(y, ek);
                                }
                            }
                        }
                        "$keys" => {
                            if let J::Array(pairs) = x {
                                for p in pairs {
                                    if let J::Array(kv) = p {
                                        if kv.len() == 2 {
                                            self.visit(&kv[0], key_kind(kind));
                                            self.visit(&kv[1], Kind::Any);
                                        }
                                    }
                                }
                            }
                        }
                        "$meta" => self.visit(x, Kind::Table),
                        _ => {
                            let key = unescape(k).unwrap_or(k);
                            let ck = match kind {
                                Kind::Any | Kind::Table | Kind::Breakdown => Kind::Any,
                                Kind::Item if !ITEM_FIELDS.contains(&key) => Kind::Any,
                                Kind::ItemList => Kind::Item,
                                _ => member_kind(kind, key),
                            };
                            self.visit(x, ck);
                        }
                    }
                }
            }
            _ => {}
        }
    }

    // ---- pass 2: allocation ----

    fn allocate(&mut self) -> Result<(), String> {
        let mut order: Vec<(NK, Kind)> = self.kinds.iter().map(|(k, v)| (*k, *v)).collect();
        // parents of immutable handles first; effects before skills
        let rank = |k: Kind| match k {
            Kind::Override => 0,
            Kind::Env => 1,
            Kind::Store => 2,
            Kind::Actor => 3,
            Kind::Item => 4,
            Kind::Effect => 5,
            Kind::Skill => 6,
            _ => 7,
        };
        order.sort_by_key(|(nk, k)| {
            (
                rank(*k),
                match nk {
                    NK::Id(i) => (0, *i as usize),
                    NK::Addr(a) => (1, *a),
                },
            )
        });
        for (nk, kind) in order {
            if self.objs.contains_key(&nk) {
                continue;
            }
            let def = self.nodes_def[&nk];
            let o = self.alloc(kind, def)?;
            self.objs.insert(nk, o);
        }
        Ok(())
    }

    fn bind(&mut self, j: Option<&'a J>, o: PObj) {
        if let Some(j) = j {
            if let Some((nk, _)) = self.node_of(j) {
                self.objs.insert(nk, o);
            }
        }
    }

    fn alloc(&mut self, kind: Kind, def: &'a J) -> Result<PObj, String> {
        Ok(match kind {
            Kind::Override => PObj::Override(Rc::new(Override::default())),
            Kind::Env => {
                let mode = match def.get("mode").and_then(J::as_str) {
                    Some("CALCS") => Mode::CALCS,
                    Some("CALCULATOR") => Mode::CALCULATOR,
                    _ => Mode::MAIN,
                };
                let ov = match def.get("override").and_then(|j| self.node_of(j)) {
                    Some((nk, _)) => match self.objs.get(&nk) {
                        Some(PObj::Override(o)) => o.clone(),
                        _ => Rc::new(Override::default()),
                    },
                    None => Rc::new(Override::default()),
                };
                let env = Env::new(&self.build, mode, ov, self.build.spec.get());
                for (k, a) in [("player", &env.player), ("enemy", &env.enemy)] {
                    self.bind(def.get(k), PObj::Actor(a.clone()));
                    // the actor's own immutable handle
                    let adef = def.get(k).and_then(|j| self.node_of(j)).map(|(_, d)| d);
                    self.bind(
                        adef.and_then(|d| d.get("itemList")),
                        PObj::ItemList(a.itemList.clone()),
                    );
                }
                for (k, t) in [
                    ("configInput", &env.configInput),
                    ("configPlaceholder", &env.configPlaceholder),
                    ("calcsInput", &env.calcsInput),
                ] {
                    t.wipe();
                    self.bind(def.get(k), PObj::Table(t.clone()));
                }
                PObj::Env(env)
            }
            Kind::Store => {
                let s = match def.get("$store").and_then(J::as_str) {
                    Some("ModDB") => ModStore::ModDB(None),
                    _ => ModStore::ModList(None),
                };
                self.bind(def.get("conditions"), PObj::Table(s.conditions.clone()));
                self.bind(def.get("multipliers"), PObj::Table(s.multipliers.clone()));
                PObj::Store(s)
            }
            Kind::Actor => {
                let a = Actor::new();
                self.bind(def.get("itemList"), PObj::ItemList(a.itemList.clone()));
                PObj::Actor(a)
            }
            Kind::Item => {
                let it: ItemRef = Rc::new(Item::default());
                self.bind(def.get("requirements"), PObj::Table(it.requirements.clone()));
                PObj::Item(it)
            }
            Kind::Effect => {
                let ge = def
                    .get("grantedEffect")
                    .and_then(|j| j.get("$data"))
                    .and_then(J::as_str)
                    .and_then(|p| match resolve_data(p) {
                        DataTarget::Skill(g) => Some(g),
                        _ => None,
                    });
                let ge = match ge {
                    Some(g) => g,
                    None => {
                        self.note(format!("skill effect without a resolvable grantedEffect: {}", short(def)));
                        any_granted_effect()
                    }
                };
                PObj::Effect(SkillEffect::new(ge, 0.0, 0.0))
            }
            Kind::Skill => {
                let eff = def.get("activeEffect").and_then(|j| self.node_of(j));
                let eff = match eff.and_then(|(nk, _)| self.objs.get(&nk)) {
                    Some(PObj::Effect(e)) => e.clone(),
                    _ => SkillEffect::new(any_granted_effect(), 0.0, 0.0),
                };
                PObj::Skill(ActiveSkill::new(eff, Shared::new(Vec::new()), Weak::new()))
            }
            Kind::Cfg => PObj::Cfg(Rc::new(ModCfg::default())),
            Kind::Gem => PObj::Gem(Rc::new(GemInstance::default())),
            Kind::Group => PObj::Group(Rc::new(SocketGroup::default())),
            Kind::Breakdown => PObj::Breakdown(Breakdown {
                t: Table::new(),
                modDB: Rc::new(ModStore::default()),
                output: Table::new(),
                actor: Weak::new(),
            }),
            Kind::SkillTypes => PObj::SkillTypes(Shared::new(SkillTypeSet::default())),
            Kind::NameSet => {
                let mut set = ahash::AHashSet::new();
                if let J::Object(m) = def {
                    for k in m.keys() {
                        if let Some(n) = unescape(k) {
                            set.insert(Name::from(n));
                        }
                    }
                }
                PObj::NameSet(Arc::new(set))
            }
            Kind::SkillList => PObj::SkillList(Shared::new(Vec::new())),
            Kind::EffectList => PObj::EffectList(Shared::new(Vec::new())),
            Kind::TableList => PObj::TableList(Shared::new(Vec::new())),
            Kind::NameList => PObj::NameList(Shared::new(Vec::new())),
            Kind::ModVec => PObj::ModVec(Shared::new(Vec::new())),
            Kind::GemList => PObj::GemList(Shared::new(Vec::new())),
            Kind::NodeMap => PObj::NodeMap(Shared::new(LjIntMap::new())),
            Kind::BoolMap => PObj::BoolMap(Shared::new(LjIntMap::new())),
            Kind::ItemFlags => PObj::ItemFlags(Shared::new(PtrMap::new())),
            Kind::GemSet => PObj::GemSet(Shared::new(PtrSet::new())),
            Kind::ItemList => PObj::ItemList(Shared::new(indexmap::IndexMap::new())),
            Kind::ItemMods => PObj::ItemMods(ModStore::ModList(None)),
            // the item fills its slot lists itself
            Kind::SlotMods => PObj::Table(Table::new()),
            _ => PObj::Table(Table::new()),
        })
    }

    // ---- decoding values ----

    fn node(&mut self, spec: bool, id: f64) -> NodeRef {
        let key = (spec, id as i64);
        if let Some(n) = self.tree_nodes.get(&key) {
            return n.clone();
        }
        let from_build = if spec {
            self.build.spec.get().nodes.with(|n| n.get(id as i64))
        } else {
            self.build.spec.get().tree.get().nodes.get(id as i64)
        };
        let n = from_build.unwrap_or_else(|| {
            let tree = Rc::new(Node {
                id,
                ..Node::default()
            });
            if spec {
                Rc::new(Node {
                    id,
                    tree: Some(tree),
                    ..Node::default()
                })
            } else {
                tree
            }
        });
        with_registry(|r| r.keep(n.clone()));
        self.tree_nodes.insert(key, n.clone());
        n
    }

    /// A placeholder of kind `k` that exports as `marker`.
    fn placeholder(&mut self, k: Kind, marker: &J) -> Val {
        let o: Option<Rc<dyn std::any::Any>> = match k {
            Kind::Actor => Some(Actor::new()),
            Kind::Skill => Some(ActiveSkill::new(
                SkillEffect::new(any_granted_effect(), 0.0, 0.0),
                Shared::new(Vec::new()),
                Weak::new(),
            )),
            Kind::Env => Some(Env::new(
                &self.build,
                Mode::MAIN,
                Rc::new(Override::default()),
                self.build.spec.get(),
            )),
            Kind::Store => Some(ModStore::ModList(None)),
            Kind::Effect => Some(SkillEffect::new(any_granted_effect(), 0.0, 0.0)),
            Kind::Cfg => Some(Rc::new(ModCfg::default())),
            Kind::Item => Some(Rc::new(Item::default())),
            Kind::Gem => Some(Rc::new(GemInstance::default())),
            Kind::Group => Some(Rc::new(SocketGroup::default())),
            _ => None,
        };
        match o {
            Some(o) => {
                with_registry(|r| r.set_foreign(o.clone(), marker.clone()));
                Val::Obj(o)
            }
            None => {
                if k == Kind::Table {
                    let t = Table::new();
                    with_registry(|r| r.set_foreign_table(&t, marker.clone()));
                    return Val::Table(t);
                }
                Val::Obj(Rc::new(Marker(marker.clone())))
            }
        }
    }

    fn decode(&mut self, j: &'a J, expect: Kind) -> Val {
        match j {
            J::Null => Val::Nil,
            J::Bool(b) => Val::Bool(*b),
            J::Number(n) => Val::Num(n.as_f64().unwrap_or(f64::NAN)),
            J::String(s) => Val::Str(Rc::from(s.as_str())),
            J::Object(m) => {
                if let Some(mk) = marker_key(m) {
                    return self.decode_marker(mk, j, expect);
                }
                if is_mod(m) {
                    return match decode_mod(j) {
                        Ok((md, _)) => Val::Mod(Arc::new(md)),
                        Err(e) => {
                            self.note(format!("undecodable mod: {e}"));
                            Val::Nil
                        }
                    };
                }
                self.obj_val(j)
            }
            J::Array(_) => self.obj_val(j),
        }
    }

    fn obj_val(&mut self, j: &'a J) -> Val {
        match self.node_of(j) {
            Some((nk, _)) => match self.objs.get(&nk) {
                Some(o) => o.to_val(),
                None => {
                    self.note(format!("unallocated table {nk:?}"));
                    Val::Nil
                }
            },
            None => Val::Nil,
        }
    }

    fn decode_marker(&mut self, mk: &str, j: &'a J, expect: Kind) -> Val {
        match mk {
            "$ref" => self.obj_val(j),
            "$float" => Val::Num(crate::schema::decode_num(j).unwrap_or(f64::NAN)),
            "$data" => {
                let path = j["$data"].as_str().unwrap_or_default();
                let target = resolve_data(path);
                let ptr = match &target {
                    DataTarget::Skill(x) => Some(Arc::as_ptr(x).cast::<()>() as usize),
                    DataTarget::Gem(x) => Some(Arc::as_ptr(x).cast::<()>() as usize),
                    DataTarget::Level(x) => Some(Arc::as_ptr(x).cast::<()>() as usize),
                    DataTarget::ItemBase(x) => Some(Arc::as_ptr(x).cast::<()>() as usize),
                    DataTarget::Minion(x) => Some(Arc::as_ptr(x).cast::<()>() as usize),
                    _ => None,
                };
                if let Some(p) = ptr {
                    with_registry(|r| r.set_data_path(p, path.to_owned()));
                }
                Val::Obj(Rc::new(DataRef {
                    path: path.to_owned(),
                    target,
                }))
            }
            "$node" | "$treeNode" => {
                let id = j[mk].as_f64().unwrap_or(0.0);
                Val::Obj(self.node(mk == "$node", id))
            }
            "$object" => match j["$object"].as_str() {
                Some("Build") => Val::Obj(self.build.clone()),
                Some("PassiveSpec") => Val::Obj(self.build.spec.get()),
                _ => Val::Obj(Rc::new(Marker(j.clone()))),
            },
            _ => {
                if expect.is_strong() && !expect.is_leaf() {
                    self.placeholder(expect, j)
                } else {
                    Val::Obj(Rc::new(Marker(j.clone())))
                }
            }
        }
    }

    // ---- pass 3: fill ----

    fn fill_all(&mut self) {
        let mut all: Vec<(NK, PObj)> = self.objs.iter().map(|(k, v)| (*k, v.clone())).collect();
        all.sort_by_key(|(nk, _)| match nk {
            NK::Id(i) => (0, *i as usize),
            NK::Addr(a) => (1, *a),
        });
        for (nk, o) in &all {
            if self.seeded.contains(nk) {
                probe::clear_for_refill(o);
            }
        }
        for (nk, o) in all {
            let Some(def) = self.nodes_def.get(&nk).copied() else {
                continue;
            };
            let kind = self.kinds[&nk];
            self.fill(&o, kind, def);
        }
    }

    fn members(def: &'a J) -> Vec<(PKeyJ<'a>, &'a J)> {
        let mut out = Vec::new();
        match def {
            J::Array(a) => {
                for (i, x) in a.iter().enumerate() {
                    out.push((PKeyJ::N((i + 1) as f64), x));
                }
            }
            J::Object(m) => {
                for (k, x) in m {
                    match k.as_str() {
                        "$seq" => {
                            if let J::Array(a) = x {
                                for (i, y) in a.iter().enumerate() {
                                    out.push((PKeyJ::N((i + 1) as f64), y));
                                }
                            }
                        }
                        "$num" => {
                            if let J::Object(n) = x {
                                for (nk, y) in n {
                                    let v = match nk.as_str() {
                                        "inf" => f64::INFINITY,
                                        "-inf" => f64::NEG_INFINITY,
                                        s => s.parse::<f64>().unwrap_or(f64::NAN),
                                    };
                                    out.push((PKeyJ::N(v), y));
                                }
                            }
                        }
                        "$keys" => {
                            if let J::Array(pairs) = x {
                                for p in pairs {
                                    if let J::Array(kv) = p {
                                        if kv.len() == 2 {
                                            out.push((PKeyJ::O(&kv[0]), &kv[1]));
                                        }
                                    }
                                }
                            }
                        }
                        "$meta" => out.push((PKeyJ::Meta, x)),
                        _ => {
                            if let Some(s) = unescape(k) {
                                out.push((PKeyJ::S(s), x));
                            }
                        }
                    }
                }
            }
            _ => {}
        }
        out
    }

    fn pkey(&mut self, k: &PKeyJ<'a>, kind: Kind) -> PKey {
        match k {
            PKeyJ::S(s) => PKey::S((*s).to_owned()),
            PKeyJ::N(n) => PKey::N(*n),
            PKeyJ::O(j) => PKey::O(PV::V(self.decode(j, key_kind(kind)))),
            PKeyJ::Meta => PKey::S("$meta".to_owned()),
        }
    }

    fn fill(&mut self, o: &PObj, kind: Kind, def: &'a J) {
        let members = Self::members(def);
        match o {
            PObj::Table(t) | PObj::Breakdown(Breakdown { t, .. }) => {
                let is_bd = matches!(o, PObj::Breakdown(_));
                for (k, x) in members {
                    if let PKeyJ::Meta = k {
                        self.fill_meta(t, x);
                        continue;
                    }
                    if is_bd {
                        if let PKeyJ::S(s) = &k {
                            let src = x.get("$fn").and_then(J::as_str);
                            if BREAKDOWN_FNS.iter().any(|(n, f)| n == s && Some(*f) == src) {
                                continue;
                            }
                        }
                    }
                    let key = self.pkey(&k, kind);
                    let v = self.decode(x, Kind::Any);
                    probe::table_set(t, &key, v);
                }
            }
            PObj::Store(s) => self.fill_store(s, def, members),
            PObj::Item(it) => self.fill_item(it, def),
            PObj::SkillTypes(st) => {
                for (k, x) in members {
                    if let (PKeyJ::N(n), J::Bool(true)) = (k, x) {
                        st.with_mut(|s| s.set(n as pob_gamedata::SkillType::T, true));
                    } else {
                        self.note("skillTypes entry that is not [n] = true".to_owned());
                    }
                }
            }
            PObj::NameSet(_) => {}
            PObj::SkillList(l) => self.fill_list(l, members, Kind::Skill),
            PObj::EffectList(l) => self.fill_list(l, members, Kind::Effect),
            PObj::TableList(l) => self.fill_list(l, members, Kind::Table),
            PObj::NameList(l) => self.fill_list(l, members, Kind::Any),
            PObj::ModVec(l) => self.fill_list(l, members, Kind::Any),
            PObj::GemList(l) => self.fill_list(l, members, Kind::Gem),
            PObj::NodeMap(m) => {
                for (k, x) in members {
                    let v = self.decode(x, Kind::Node);
                    match (k, v.obj::<Node>()) {
                        (PKeyJ::N(n), Some(node)) => m.with_mut(|m| m.set(n as i64, Some(node))),
                        _ => self.note("allocNodes-like map entry that is not [n] = node".to_owned()),
                    }
                }
            }
            PObj::BoolMap(m) => {
                for (k, x) in members {
                    match (k, x) {
                        (PKeyJ::N(n), J::Bool(b)) => m.with_mut(|m| m.set(n as i64, Some(*b))),
                        _ => self.note("bool map entry that is not [n] = bool".to_owned()),
                    }
                }
            }
            PObj::ItemFlags(m) => {
                for (k, x) in members {
                    let (PKeyJ::O(kj), J::Bool(b)) = (k, x) else {
                        self.note("item map entry that is not [item] = bool".to_owned());
                        continue;
                    };
                    match self.decode(kj, Kind::Item).obj::<Item>() {
                        Some(it) => m.with_mut(|m| m.set(&it, Some(*b))),
                        None => self.note("item map key that is not an item".to_owned()),
                    }
                }
            }
            PObj::GemSet(m) => {
                for (k, _) in members {
                    let PKeyJ::O(kj) = k else { continue };
                    match self.decode(kj, Kind::Gem).obj::<GemInstance>() {
                        Some(g) => {
                            m.with_mut(|m| m.insert(&g));
                        }
                        None => self.note("isSupporting key that is not a gem".to_owned()),
                    }
                }
            }
            PObj::ItemList(m) => {
                for (k, x) in members {
                    let PKeyJ::S(s) = k else { continue };
                    match self.decode(x, Kind::Item).obj::<Item>() {
                        Some(it) => m.with_mut(|m| {
                            m.insert(Name::from(s), it);
                        }),
                        None => self.note(format!("itemList.{s} is not an item")),
                    }
                }
            }
            PObj::ItemMods(s) => {
                for (_, x) in members {
                    match decode_mod(x) {
                        Ok((m, at)) => {
                            let m = Arc::new(m);
                            if let Some(at) = at {
                                with_registry(|r| r.set_mod_at(s, &m, at));
                            }
                            s.AddMod(m);
                        }
                        Err(e) => self.note(format!("item mod: {e}")),
                    }
                }
            }
            _ => {
                // reflected structs
                let ptr = o.reg_ptr();
                let mut present: Vec<&str> = Vec::new();
                let mut fell_back: Vec<&str> = Vec::new();
                for (k, x) in members {
                    match k {
                        PKeyJ::S(s) => {
                            if kind == Kind::Env && (s == "mode" || s == "data") {
                                present.push(s);
                                continue;
                            }
                            let v = self.decode(x, member_kind(kind, s));
                            // a typed field that could not take the value stays absent
                            if set_member(o, &PKey::S(s.to_owned()), v) {
                                present.push(s);
                            } else {
                                fell_back.push(s);
                            }
                        }
                        PKeyJ::Meta => self.note(format!("{kind:?} with a metatable")),
                        other => {
                            let key = self.pkey(&other, kind);
                            let v = self.decode(x, Kind::Any);
                            set_member(o, &key, v);
                        }
                    }
                }
                if let (PObj::Skill(sk), Some(u)) = (o, def.get("$uuid").and_then(J::as_str)) {
                    with_registry(|r| r.set_uuid(sk, u.to_owned()));
                }
                let seeded = self.seeded_keys.contains(&o.key());
                for k in typed_keys(kind) {
                    if fell_back.contains(k) {
                        continue;
                    }
                    if present.contains(k) {
                        probe::unmark_absent(ptr, k);
                    } else if seeded {
                        probe::unset_typed(o, k);
                    } else {
                        mark_absent(ptr, k);
                    }
                }
                if kind == Kind::Skill && !present.contains(&"skillFlags") {
                    mark_absent(ptr, "skillFlags");
                }
                with_registry(|r| r.keep(o.to_val().obj_any()));
            }
        }
    }

    fn fill_list<T: ProbeField + Clone>(&mut self, l: &Shared<Vec<T>>, members: Vec<(PKeyJ<'a>, &'a J)>, ek: Kind) {
        let mut items: Vec<(f64, T)> = Vec::new();
        for (k, x) in members {
            let PKeyJ::N(n) = k else {
                self.note("list entry with a non-number key".to_owned());
                continue;
            };
            let v = self.decode(x, ek);
            match T::from_val(&v) {
                Some(t) => items.push((n, t)),
                None => self.note(format!("list element of the wrong type: {}", short(x))),
            }
        }
        items.sort_by(|a, b| a.0.total_cmp(&b.0));
        l.with_mut(|v| v.extend(items.into_iter().map(|(_, t)| t)));
    }

    fn fill_meta(&mut self, t: &Table, meta: &'a J) {
        let idx = match meta {
            J::Object(m) => m.get("__index"),
            _ => None,
        };
        let Some(idx) = idx else {
            self.note("metatable without __index".to_owned());
            return;
        };
        if let Some(src) = idx.get("$fn") {
            let _ = src;
            // the closure's upvalues are not in the dump: an empty OrChain that
            // exports as the same marker
            with_registry(|r| r.set_meta_fn(t, idx.clone()));
            t.set_meta(Some(IndexMeta::OrChain(smallvec::SmallVec::new())));
            return;
        }
        match self.decode(idx, Kind::Table).table() {
            Some(p) => t.set_meta(Some(IndexMeta::Table(p))),
            None => self.note("__index that is neither a table nor a function".to_owned()),
        }
    }

    fn fill_store(&mut self, s: &ModStoreRef, def: &'a J, members: Vec<(PKeyJ<'a>, &'a J)>) {
        let ptr = rc_ptr(s);
        match def.get("mods") {
            Some(J::Object(byname)) => {
                for (name, list) in byname {
                    let name = Name::from(unescape(name).unwrap_or(name));
                    let mut mods = Vec::new();
                    if let J::Array(a) = list {
                        for x in a {
                            match decode_mod(x) {
                                Ok((m, at)) => {
                                    let m = Arc::new(m);
                                    if let Some(at) = at {
                                        with_registry(|r| r.set_mod_at(s, &m, at));
                                    }
                                    mods.push(m);
                                }
                                Err(e) => self.note(format!("store mod: {e}")),
                            }
                        }
                    }
                    probe::store_put_name(s, name, mods);
                }
            }
            Some(J::Array(a)) => {
                for x in a {
                    match decode_mod(x) {
                        Ok((m, at)) => {
                            let m = Arc::new(m);
                            if let Some(at) = at {
                                with_registry(|r| r.set_mod_at(s, &m, at));
                            }
                            s.AddMod(m);
                        }
                        Err(e) => self.note(format!("store mod: {e}")),
                    }
                }
            }
            _ => {}
        }
        let mut present = Vec::new();
        let o = PObj::Store(s.clone());
        for (k, x) in members {
            match k {
                PKeyJ::S("mods") => {}
                PKeyJ::S(key) => {
                    present.push(key);
                    let v = self.decode(x, member_kind(Kind::Store, key));
                    if !set_member(&o, &PKey::S(key.to_owned()), v) {
                        self.note(format!("modStore.{key} not set"));
                    }
                }
                _ => self.note("mod store with a non-string member".to_owned()),
            }
        }
        for k in ["actor", "parent", "conditions", "multipliers"] {
            if present.contains(&k) {
                probe::unmark_absent(ptr, k);
            } else {
                mark_absent(ptr, k);
            }
        }
        with_registry(|r| r.keep(s.clone()));
    }

    fn fill_item(&mut self, it: &ItemRef, def: &'a J) {
        let ptr = rc_ptr(it);
        let J::Object(m) = def else { return };
        let get = |k: &str| -> Option<&'a J> { m.get(k) };
        let s = |k: &str| get(k).and_then(J::as_str).map(Rc::<str>::from);
        let n = |k: &str| get(k).and_then(J::as_f64);
        it.id.set(n("id"));
        if let Some(x) = s("name") {
            it.name.set(x);
        }
        it.title.set(s("title"));
        it.baseName.set(s("baseName").map(|x| Name::from(&x)));
        if let Some(x) = s("type") {
            it.r#type.set(Name::from(&x));
        }
        if let Some(x) = s("rarity") {
            it.rarity.set(Name::from(&x));
        }
        it.quality.set(n("quality"));
        it.corrupted.set(get("corrupted").and_then(J::as_bool).unwrap_or(false));
        it.variant.set(n("variant"));
        it.jewelRadiusIndex.set(n("jewelRadiusIndex"));
        it.limit.set(n("limit"));
        if let Some(b) = get("base") {
            match self.decode(b, Kind::Data).obj::<DataRef>().map(|d| d.target.clone()) {
                Some(DataTarget::ItemBase(x)) => it.base.set(Some(x)),
                _ => self.note(format!("item base not resolved: {b}")),
            }
        }
        for (k, f) in [
            ("weaponData", &it.weaponData),
            ("armourData", &it.armourData),
            ("flaskData", &it.flaskData),
            ("tinctureData", &it.tinctureData),
            ("jewelData", &it.jewelData),
        ] {
            if let Some(x) = get(k) {
                f.set(self.decode(x, Kind::Table).table());
            }
        }
        if let Some(x) = get("modList") {
            match self.decode(x, Kind::ItemMods).obj::<ModStore>() {
                Some(l) => it.modList.set(Some(l)),
                None => self.note("item.modList not decoded".to_owned()),
            }
        }
        if let Some(x) = get("slotModList") {
            let members = Self::members(x);
            for (k, y) in members {
                let PKeyJ::N(i) = k else { continue };
                let l = self.decode(y, Kind::ItemMods).obj::<ModStore>();
                match (i as usize, l) {
                    (1, Some(l)) => it.slotModList[0].set(Some(l)),
                    (2, Some(l)) => it.slotModList[1].set(Some(l)),
                    // CCR-0030: Item.slotModList has two slots; Lua writes [3] (Item.lua:2699)
                    (3, Some(l)) => with_registry(|r| {
                        r.set_slot3(it, l);
                        r.fallback("item.slotModList[3] (CCR-0030)".to_owned());
                    }),
                    _ => self.note("item.slotModList entry not decoded".to_owned()),
                }
            }
        }
        for k in ITEM_FIELDS {
            if m.contains_key(*k) {
                probe::unmark_absent(ptr, k);
            } else {
                mark_absent(ptr, k);
            }
        }
        with_registry(|r| r.keep(it.clone()));
    }

    /// `actor.breakdown` closures capture (modDB, output, actor): rebuild them
    /// from the owning actor.
    fn patch_breakdowns(&mut self) {
        let actors: Vec<ActorRef> = self
            .objs
            .values()
            .filter_map(|o| match o {
                PObj::Actor(a) => Some(a.clone()),
                _ => None,
            })
            .collect();
        for a in actors {
            if let Some(b) = a.breakdown.get() {
                let nb = Breakdown {
                    t: b.t.clone(),
                    modDB: a.modDB.get(),
                    output: a.output.get(),
                    actor: Rc::downgrade(&a),
                };
                a.breakdown.set(Some(nb.clone()));
                self.breakdowns.insert(nb.t.as_ptr() as usize, nb);
            }
        }
    }

    fn patched_breakdown(&self, v: Val) -> Val {
        if let Some(b) = v.obj::<Breakdown>() {
            if let Some(nb) = self.breakdowns.get(&(b.t.as_ptr() as usize)) {
                return nb.to_val();
            }
        }
        v
    }

    fn fill_global_cache(&mut self, gc: &'a J) {
        let Some(J::Object(modes)) = gc.get("cachedData") else {
            return;
        };
        for (mode, entries) in modes {
            let mode = match mode.as_str() {
                "MAIN" => Mode::MAIN,
                "CALCS" => Mode::CALCS,
                "CALCULATOR" => Mode::CALCULATOR,
                _ => continue,
            };
            let J::Object(entries) = entries else { continue };
            for (uuid, e) in entries {
                let skill = e
                    .get("ActiveSkill")
                    .map(|j| self.decode(j, Kind::Skill))
                    .and_then(|v| v.obj::<ActiveSkill>());
                let env = e
                    .get("Env")
                    .map(|j| self.decode(j, Kind::Env))
                    .and_then(|v| v.obj::<Env>());
                let (Some(skill), Some(env)) = (skill, env) else {
                    self.note(format!("GlobalCache entry {uuid} without skill/env"));
                    continue;
                };
                let mut v = |k: &str| match e.get(k) {
                    Some(j) => self.decode(j, Kind::Any),
                    None => Val::Nil,
                };
                let entry = CachedData {
                    Name: Name::from(v("Name").str().as_deref().unwrap_or("")),
                    Speed: v("Speed"),
                    HitSpeed: v("HitSpeed"),
                    ManaCost: v("ManaCost"),
                    LifeCost: v("LifeCost"),
                    ESCost: v("ESCost"),
                    RageCost: v("RageCost"),
                    HitChance: v("HitChance"),
                    AccuracyHitChance: v("AccuracyHitChance"),
                    PreEffectiveCritChance: v("PreEffectiveCritChance"),
                    CritChance: v("CritChance"),
                    TotalDPS: v("TotalDPS"),
                    ActiveSkill: skill,
                    Env: env,
                };
                set_cachedData(mode, Name::from(uuid.as_str()), Some(Rc::new(entry)));
            }
        }
    }
}

/// A member key as dumped.
enum PKeyJ<'a> {
    S(&'a str),
    N(f64),
    O(&'a J),
    Meta,
}

trait ObjAny {
    fn obj_any(&self) -> Rc<dyn std::any::Any>;
}
impl ObjAny for Val {
    fn obj_any(&self) -> Rc<dyn std::any::Any> {
        match self {
            Val::Obj(o) => o.clone(),
            _ => Rc::new(()),
        }
    }
}

fn short(j: &J) -> String {
    let s = j.to_string();
    if s.len() > 160 {
        format!("{}…", &s[..s.char_indices().nth(160).map_or(s.len(), |(i, _)| i)])
    } else {
        s
    }
}

fn any_granted_effect() -> pob_gamedata::GrantedEffectRef {
    pob_gamedata::data()
        .skills
        .values()
        .next()
        .cloned()
        .expect("data.skills is empty")
}

// ---- data paths ----------------------------------------------------------------------------

/// Split a Lua-like path into steps: `data.skills.X.levels[3]`, `data.gems["…"]`.
fn path_steps(p: &str) -> Option<Vec<String>> {
    let mut out = Vec::new();
    let b = p.as_bytes();
    let mut i = 0;
    // the root identifier
    while i < b.len() && b[i] != b'.' && b[i] != b'[' {
        i += 1;
    }
    out.push(p[..i].to_owned());
    while i < b.len() {
        match b[i] {
            b'.' => {
                let s = i + 1;
                i = s;
                while i < b.len() && b[i] != b'.' && b[i] != b'[' {
                    i += 1;
                }
                out.push(p[s..i].to_owned());
            }
            b'[' => {
                if b.get(i + 1) == Some(&b'"') {
                    // a JSON string up to the matching quote
                    let rest = &p[i + 1..];
                    let mut de = serde_json::Deserializer::from_str(rest).into_iter::<String>();
                    let s = de.next()?.ok()?;
                    let used = de.byte_offset();
                    out.push(s);
                    i = i + 1 + used;
                    if b.get(i) != Some(&b']') {
                        return None;
                    }
                    i += 1;
                } else {
                    let s = i + 1;
                    while i < b.len() && b[i] != b']' {
                        i += 1;
                    }
                    out.push(p[s..i].to_owned());
                    i += 1;
                }
            }
            _ => return None,
        }
    }
    Some(out)
}

/// The typed target of a `$data` path.
pub fn resolve_data(path: &str) -> DataTarget {
    if !pob_gamedata::is_initialised() {
        return DataTarget::Other;
    }
    let d = pob_gamedata::data();
    let Some(steps) = path_steps(path) else {
        return DataTarget::Other;
    };
    let s: Vec<&str> = steps.iter().map(String::as_str).collect();
    match s.as_slice() {
        ["data"] => DataTarget::Root,
        ["data", "skills", id] => d
            .skills
            .get(&Name::from(id))
            .map_or(DataTarget::Other, |g| DataTarget::Skill(g.clone())),
        ["data", "skills", id, "levels", n] => d
            .skills
            .get(&Name::from(id))
            .and_then(|g| g.levels.get(n.parse::<f64>().ok()?).cloned())
            .map_or(DataTarget::Other, DataTarget::Level),
        ["data", "gems", id] => d
            .gems
            .get(&Name::from(id))
            .map_or(DataTarget::Other, |g| DataTarget::Gem(g.clone())),
        ["data", "itemBases", id] => d
            .itemBases
            .get(*id)
            .map_or(DataTarget::Other, |g| DataTarget::ItemBase(g.clone())),
        ["data", "minions", id] => d
            .minions
            .get(&Name::from(id))
            .map_or(DataTarget::Other, |g| DataTarget::Minion(g.clone())),
        ["data", "spectres", id] => d
            .spectres
            .get(&Name::from(id))
            .map_or(DataTarget::Other, |g| DataTarget::Minion(g.clone())),
        _ => DataTarget::Other,
    }
}

/// A mod as `ModRef` (for callers building stores from dumps).
pub fn decode_mod_ref(j: &J) -> Result<ModRef, String> {
    decode_mod(j).map(|(m, _)| Arc::new(m))
}

pub use crate::mod_json::{decode_mod, decode_tag, j_to_value};

/// Re-export for tests: the `SBox` wrapper of `Shared` values in tables.
pub type SharedBox<T> = SBox<T>;
