//! Replay of not-yet-ported callees (calc doc §9.2, "Replay").
//!
//! Owner: ORC-X. A [`Replay`] swaps a callee (PF-G calling `offence`, OF-A
//! calling skill hooks through `runSkillFunc`, triggers calling
//! `buildActiveSkill`, …) for its recorded `in → out` state **delta**, keyed by
//! hit id (`offence:in#4`; a function probe's `:in` and `:out` share `n`).
//!
//! [`Replay::apply`] leaves the caller's state as the caller left it and
//! applies only what the callee changed:
//!
//! 1. the recorded `:in` and `:out` graphs are matched table by table along
//!    their paths (`env.player.output` in `:out` is the table at
//!    `env.player.output` in `:in`; a table first reached through a path the
//!    other dump does not have, or of another kind — store class, item,
//!    skill uuid — is new);
//! 2. per matched table the delta is: keys added, changed (a different scalar
//!    or marker, or a reference to another table) or removed, the `$keys`
//!    part when it differs, and per mod store the mods added and removed
//!    (multisets per name, as `formatMod` + `$at` JSON);
//! 3. the live state is exported (`pob_calc::probe::export_root`) and the
//!    `:in` tables are located in it the same way; the delta is applied to
//!    the located tables (added mods go to the end of their list, as AddMod
//!    puts them; tables the callee created are copied in, with references to
//!    pre-existing tables translated to the live ones);
//! 4. the merged state is written back onto the live graph in place
//!    (`crate::inject::apply_onto`), so every handle the caller holds stays
//!    valid and every path the callee did not touch keeps the caller's value.
//!
//! Limits (dumps carry no identity across hits): a table the callee
//! *replaced* by a fresh one at the same path is treated as modified in place
//! (keys the caller added that the callee's fresh table would not have
//! survive); a delta whose `:in` table the caller no longer has at that path
//! is skipped and reported in [`Applied::notes`].

use std::collections::{BTreeMap, BTreeSet, HashMap, VecDeque};

use serde_json::{Map, Value as J};

use crate::inject::{is_mod, marker_key};
use crate::schema::StateDump;

/// One recorded call of the callee.
#[derive(Clone)]
pub struct Call {
    pub din: StateDump,
    pub dout: StateDump,
}

/// Recorded callee calls for one build run.
#[derive(Default)]
pub struct Replay {
    /// hit id of the callee's `:in` → its `:in` and `:out` dumps.
    pub calls: BTreeMap<String, Call>,
}

/// What [`Replay::apply_report`] did.
#[derive(Debug, Default)]
pub struct Applied {
    /// Tables of the delta (matched `:in`/`:out` tables with a change).
    pub changed_tables: usize,
    /// Keys set or removed, mods added or removed.
    pub edits: usize,
    /// Deltas that could not be placed (the caller lacks the `:in` table).
    pub notes: Vec<String>,
    /// The callee's recorded return value (`:out`'s `ret` local: the packed
    /// results `{n = k, …}`), decoded into the live graph.
    pub ret: Option<pob_calc::Val>,
}

impl Replay {
    /// Load the recorded `:in`/`:out` pairs of `callee` (a function probe id,
    /// e.g. `offence`, `runSkillFunc`, `skill:Barrage:preDamageFunc`) from a
    /// build run's dumps, keyed by the `:in` hit id.
    pub fn load(callee: &str, dumps: &BTreeMap<String, StateDump>) -> Replay {
        let probe_in = format!("{callee}:in");
        let probe_out = format!("{callee}:out");
        let mut calls = BTreeMap::new();
        for d in dumps.values() {
            if d.probe != probe_in {
                continue;
            }
            let out = dumps
                .values()
                .find(|o| o.probe == probe_out && o.n == d.n && o.env_id == d.env_id);
            if let Some(out) = out {
                calls.insert(
                    d.hit.clone(),
                    Call {
                        din: d.clone(),
                        dout: out.clone(),
                    },
                );
            }
        }
        Replay { calls }
    }

    /// The `:out` dump recorded for `hit`.
    pub fn out_of(&self, hit: &str) -> Option<&StateDump> {
        self.calls.get(hit).map(|c| &c.dout)
    }

    /// The `:in` dump recorded for `hit`.
    pub fn in_of(&self, hit: &str) -> Option<&StateDump> {
        self.calls.get(hit).map(|c| &c.din)
    }

    /// Apply the delta recorded for `hit` to the live Rust state (false when
    /// the hit was not recorded or the state could not be applied).
    pub fn apply(&self, hit: &str, env: &pob_calc::env::EnvRef) -> bool {
        self.apply_report(hit, env).is_ok()
    }

    /// [`Replay::apply`] with a report.
    pub fn apply_report(&self, hit: &str, env: &pob_calc::env::EnvRef) -> Result<Applied, String> {
        let call = self
            .calls
            .get(hit)
            .ok_or_else(|| format!("{hit}: not recorded"))?;
        let live = pob_calc::probe::export_root(Some(env), &[]);
        let (merged, mut applied) = merge(&live, &call.din.root, &call.dout.root);
        let dump = StateDump {
            root: merged,
            ..call.dout.clone()
        };
        let mut inj = crate::inject::apply_onto(&dump, &env.build, env)?;
        applied.ret = inj.locals.remove("ret");
        Ok(applied)
    }
}

/// `live ⊕ (out − in)` on pob-state/1 roots (see the module doc).
pub fn merge(live: &J, din: &J, dout: &J) -> (J, Applied) {
    let gi = G::decode(din);
    let go = G::decode(dout);
    let mut gl = G::decode(live);
    let oi = pair(&go, &gi);
    let il = pair(&gi, &gl);
    let mut m = Merger {
        go: &go,
        gi: &gi,
        oi: &oi,
        il: &il,
        copies: HashMap::new(),
        applied: Applied::default(),
    };
    for (o, i) in oi.iter().enumerate() {
        if let Some(i) = *i {
            m.apply_node(&mut gl, o, i);
        }
    }
    // the merged root's locals: only the callee's return value
    let ret = match go.nodes[go.root].strs.get("locals") {
        Some(V::Node(l)) => go.nodes[*l].strs.get("ret").cloned(),
        _ => None,
    };
    let mut locals = Node::default();
    if let Some(r) = ret {
        let r = m.tr(&mut gl, &r);
        locals.strs.insert("ret".to_owned(), r);
    }
    let ln = gl.alloc();
    gl.nodes[ln] = locals;
    let root = gl.root;
    gl.nodes[root].strs.insert("locals".to_owned(), V::Node(ln));
    let applied = m.applied;
    (gl.encode(), applied)
}

// ---- the decoded graph ----------------------------------------------------------------------------

type NodeId = usize;

#[derive(Clone, Debug, PartialEq)]
enum V {
    Leaf(J),
    Node(NodeId),
}

#[derive(Clone, Debug, PartialEq)]
enum Mods {
    /// ModDB: mods per (encoded) name.
    Db(BTreeMap<String, Vec<J>>),
    /// ModList: mods in order.
    List(Vec<J>),
    /// Anything else (kept verbatim).
    Raw(J),
}

/// A member key: string key, numeric key (canonical number text), metatable.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
enum MK {
    S(String),
    N(String),
    Meta,
}

#[derive(Clone, Debug, Default)]
struct Node {
    /// Reserved data members (`$store`, `$item`, `$uuid`, any other `$x`).
    head: BTreeMap<String, J>,
    mods: Option<Mods>,
    strs: BTreeMap<String, V>,
    nums: BTreeMap<String, V>,
    keys: Vec<(V, V)>,
    meta: Option<V>,
}

impl Node {
    fn get(&self, k: &MK) -> Option<&V> {
        match k {
            MK::S(s) => self.strs.get(s),
            MK::N(n) => self.nums.get(n),
            MK::Meta => self.meta.as_ref(),
        }
    }

    fn set(&mut self, k: &MK, v: V) {
        match k {
            MK::S(s) => {
                self.strs.insert(s.clone(), v);
            }
            MK::N(n) => {
                self.nums.insert(n.clone(), v);
            }
            MK::Meta => self.meta = Some(v),
        }
    }

    fn remove(&mut self, k: &MK) {
        match k {
            MK::S(s) => {
                self.strs.remove(s);
            }
            MK::N(n) => {
                self.nums.remove(n);
            }
            MK::Meta => self.meta = None,
        }
    }

    /// Length of the sequence part (`1..n` all present).
    fn seq_len(&self) -> usize {
        let mut n = 0;
        while self.nums.contains_key(&(n + 1).to_string()) {
            n += 1;
        }
        n
    }

    /// Numeric keys in emission order: the sequence, then the others
    /// ascending.
    fn num_order(&self) -> (usize, Vec<&String>) {
        let n = self.seq_len();
        let mut rest: Vec<&String> = self
            .nums
            .keys()
            .filter(|k| {
                !k.parse::<usize>()
                    .is_ok_and(|i| (1..=n).contains(&i) && i.to_string() == **k)
            })
            .collect();
        rest.sort_by(|a, b| num_of(a).total_cmp(&num_of(b)).then_with(|| a.cmp(b)));
        (n, rest)
    }

    /// Members in state_dump.lua's walk order: string keys, `$seq`, `$num`,
    /// `$keys` (key, value), `$meta`.
    fn walk_order(&self) -> Vec<&V> {
        let mut out: Vec<&V> = self.strs.values().collect();
        let (n, rest) = self.num_order();
        for i in 1..=n {
            out.push(&self.nums[&i.to_string()]);
        }
        for k in rest {
            out.push(&self.nums[k]);
        }
        for (k, v) in &self.keys {
            out.push(k);
            out.push(v);
        }
        if let Some(m) = &self.meta {
            out.push(m);
        }
        out
    }

    /// Path members (what a path step can name; `$keys` entries are not).
    fn path_members(&self) -> Vec<(MK, &V)> {
        let mut out: Vec<(MK, &V)> = self
            .strs
            .iter()
            .map(|(k, v)| (MK::S(k.clone()), v))
            .collect();
        out.extend(self.nums.iter().map(|(k, v)| (MK::N(k.clone()), v)));
        if let Some(m) = &self.meta {
            out.push((MK::Meta, m));
        }
        out
    }

    /// Tables of different kinds never match: store class, item, skill uuid.
    fn compatible(&self, o: &Node) -> bool {
        let tag = |n: &Node| {
            (
                n.head.get("$store").cloned(),
                n.head.contains_key("$item"),
                n.mods.is_some(),
            )
        };
        if tag(self) != tag(o) {
            return false;
        }
        match (self.head.get("$uuid"), o.head.get("$uuid")) {
            (Some(a), Some(b)) => a == b,
            _ => true,
        }
    }
}

fn num_of(k: &str) -> f64 {
    k.parse::<f64>().unwrap_or(f64::NAN)
}

struct G {
    nodes: Vec<Node>,
    root: NodeId,
}

struct Dec {
    nodes: Vec<Node>,
    by_id: HashMap<u64, NodeId>,
}

impl Dec {
    fn alloc(&mut self) -> NodeId {
        self.nodes.push(Node::default());
        self.nodes.len() - 1
    }

    fn id_node(&mut self, id: u64) -> NodeId {
        if let Some(&n) = self.by_id.get(&id) {
            return n;
        }
        let n = self.alloc();
        self.by_id.insert(id, n);
        n
    }

    fn val(&mut self, j: &J) -> V {
        match j {
            J::Array(a) => {
                let n = self.alloc();
                let mut node = Node::default();
                for (i, x) in a.iter().enumerate() {
                    let v = self.val(x);
                    node.nums.insert((i + 1).to_string(), v);
                }
                self.nodes[n] = node;
                V::Node(n)
            }
            J::Object(m) => {
                if m.len() == 1 {
                    if let Some(id) = m.get("$ref").and_then(J::as_u64) {
                        return V::Node(self.id_node(id));
                    }
                }
                if marker_key(m).is_some() || is_mod(m) {
                    return V::Leaf(j.clone());
                }
                let n = match m.get("$id").and_then(J::as_u64) {
                    Some(id) => self.id_node(id),
                    None => self.alloc(),
                };
                let node = self.table(m);
                self.nodes[n] = node;
                V::Node(n)
            }
            _ => V::Leaf(j.clone()),
        }
    }

    fn table(&mut self, m: &Map<String, J>) -> Node {
        let mut node = Node::default();
        let store = m.get("$store").and_then(J::as_str);
        for (k, x) in m {
            match k.as_str() {
                "$id" => {}
                "$seq" => {
                    if let J::Array(a) = x {
                        for (i, y) in a.iter().enumerate() {
                            let v = self.val(y);
                            node.nums.insert((i + 1).to_string(), v);
                        }
                    }
                }
                "$num" => {
                    if let J::Object(n) = x {
                        for (nk, y) in n {
                            let v = self.val(y);
                            node.nums.insert(nk.clone(), v);
                        }
                    }
                }
                "$keys" => {
                    if let J::Array(a) = x {
                        for pair in a {
                            if let J::Array(kv) = pair {
                                if kv.len() == 2 {
                                    let kk = self.val(&kv[0]);
                                    let vv = self.val(&kv[1]);
                                    node.keys.push((kk, vv));
                                }
                            }
                        }
                    }
                }
                "$meta" => node.meta = Some(self.val(x)),
                "mods" if store.is_some() => {
                    node.mods = Some(match (store, x) {
                        (Some("ModDB"), J::Object(byname)) => Mods::Db(
                            byname
                                .iter()
                                .map(|(n, l)| {
                                    (n.clone(), l.as_array().cloned().unwrap_or_default())
                                })
                                .collect(),
                        ),
                        (Some("ModList"), J::Array(l)) => Mods::List(l.clone()),
                        (Some("ModList"), J::Object(o)) if o.is_empty() => Mods::List(Vec::new()),
                        _ => Mods::Raw(x.clone()),
                    });
                }
                _ => {
                    if let Some(rest) = k.strip_prefix("$$") {
                        let v = self.val(x);
                        node.strs.insert(format!("${rest}"), v);
                    } else if k.starts_with('$') {
                        node.head.insert(k.clone(), x.clone());
                    } else {
                        let v = self.val(x);
                        node.strs.insert(k.clone(), v);
                    }
                }
            }
        }
        node
    }
}

impl G {
    fn decode(root: &J) -> G {
        let mut d = Dec {
            nodes: Vec::new(),
            by_id: HashMap::new(),
        };
        let root = match d.val(root) {
            V::Node(n) => n,
            V::Leaf(_) => d.alloc(),
        };
        G {
            nodes: d.nodes,
            root,
        }
    }

    fn alloc(&mut self) -> NodeId {
        self.nodes.push(Node::default());
        self.nodes.len() - 1
    }

    /// Encode as state_dump.lua does: every table in full at its canonical
    /// (breadth-first) slot, `"$id"` when referenced more than once, `$ref`
    /// elsewhere; ids in emission order.
    fn encode(&self) -> J {
        let n = self.nodes.len();
        let mut canon: Vec<Option<(NodeId, usize)>> = vec![None; n];
        let mut count = vec![0u32; n];
        canon[self.root] = Some((usize::MAX, 0));
        count[self.root] = 1;
        let mut queue = VecDeque::from([self.root]);
        while let Some(t) = queue.pop_front() {
            for (ord, v) in self.nodes[t].walk_order().into_iter().enumerate() {
                if let V::Node(c) = v {
                    count[*c] += 1;
                    if canon[*c].is_none() {
                        canon[*c] = Some((t, ord));
                        queue.push_back(*c);
                    }
                }
            }
        }
        let mut e = Enc {
            g: self,
            canon,
            count,
            ids: vec![None; n],
            next_id: 1,
            emitted: vec![false; n],
        };
        e.node(self.root, (usize::MAX, 0))
    }
}

struct Enc<'a> {
    g: &'a G,
    canon: Vec<Option<(NodeId, usize)>>,
    count: Vec<u32>,
    ids: Vec<Option<u64>>,
    next_id: u64,
    emitted: Vec<bool>,
}

impl Enc<'_> {
    fn id(&mut self, t: NodeId) -> u64 {
        if let Some(id) = self.ids[t] {
            return id;
        }
        let id = self.next_id;
        self.next_id += 1;
        self.ids[t] = Some(id);
        id
    }

    fn val(&mut self, v: &V, slot: (NodeId, usize)) -> J {
        match v {
            V::Leaf(j) => j.clone(),
            V::Node(c) => self.node(*c, slot),
        }
    }

    fn node(&mut self, t: NodeId, slot: (NodeId, usize)) -> J {
        if self.canon[t] != Some(slot) || self.emitted[t] {
            let id = self.id(t);
            return serde_json::json!({ "$ref": id });
        }
        self.emitted[t] = true;
        let id = (self.count[t] > 1).then(|| self.id(t));
        let g = self.g;
        let node = &g.nodes[t];
        let (n, rest) = node.num_order();
        let array = id.is_none()
            && node.head.is_empty()
            && node.mods.is_none()
            && node.strs.is_empty()
            && node.keys.is_empty()
            && node.meta.is_none()
            && n > 0
            && rest.is_empty();
        let mut ord = 0usize;
        let next = |o: &mut usize| {
            let s = (t, *o);
            *o += 1;
            s
        };
        if array {
            let mut a = Vec::with_capacity(n);
            for i in 1..=n {
                let s = next(&mut ord);
                a.push(self.val(&node.nums[&i.to_string()], s));
            }
            return J::Array(a);
        }
        let mut m = Map::new();
        if let Some(id) = id {
            m.insert("$id".into(), J::from(id));
        }
        for (k, x) in &node.head {
            m.insert(k.clone(), x.clone());
        }
        if let Some(mods) = &node.mods {
            let j = match mods {
                Mods::Db(byname) => J::Object(
                    byname
                        .iter()
                        .map(|(k, l)| (k.clone(), J::Array(l.clone())))
                        .collect(),
                ),
                Mods::List(l) => J::Array(l.clone()),
                Mods::Raw(j) => j.clone(),
            };
            m.insert("mods".into(), j);
        }
        for (k, v) in &node.strs {
            let s = next(&mut ord);
            let key = if k.starts_with('$') {
                format!("${k}")
            } else {
                k.clone()
            };
            let j = self.val(v, s);
            m.insert(key, j);
        }
        if n > 0 {
            let mut seq = Vec::with_capacity(n);
            for i in 1..=n {
                let s = next(&mut ord);
                seq.push(self.val(&node.nums[&i.to_string()], s));
            }
            m.insert("$seq".into(), J::Array(seq));
        }
        if !rest.is_empty() {
            let mut num = Map::new();
            for k in rest {
                let s = next(&mut ord);
                num.insert(k.clone(), self.val(&node.nums[k], s));
            }
            m.insert("$num".into(), J::Object(num));
        }
        if !node.keys.is_empty() {
            let mut keys = Vec::with_capacity(node.keys.len());
            for (k, v) in &node.keys {
                let sk = next(&mut ord);
                let jk = self.val(k, sk);
                let sv = next(&mut ord);
                let jv = self.val(v, sv);
                keys.push(J::Array(vec![jk, jv]));
            }
            m.insert("$keys".into(), J::Array(keys));
        }
        if let Some(meta) = &node.meta {
            let s = next(&mut ord);
            m.insert("$meta".into(), self.val(meta, s));
        }
        J::Object(m)
    }
}

// ---- matching tables along paths --------------------------------------------------------------------

/// For every table of `a`, the table of `b` at the same path (breadth-first
/// from the roots, so the shortest path decides; one-to-one; `locals` of the
/// roots are not followed: they are the callee's parameters, not state the
/// caller sees).
fn pair(a: &G, b: &G) -> Vec<Option<NodeId>> {
    let mut map: Vec<Option<NodeId>> = vec![None; a.nodes.len()];
    let mut used = vec![false; b.nodes.len()];
    map[a.root] = Some(b.root);
    used[b.root] = true;
    let mut queue = VecDeque::from([(a.root, b.root)]);
    while let Some((x, y)) = queue.pop_front() {
        let (na, nb) = (&a.nodes[x], &b.nodes[y]);
        for (k, va) in na.path_members() {
            if x == a.root && k == MK::S("locals".into()) {
                continue;
            }
            let (V::Node(cx), Some(V::Node(cy))) = (va, nb.get(&k)) else {
                continue;
            };
            if map[*cx].is_some() || used[*cy] || !a.nodes[*cx].compatible(&b.nodes[*cy]) {
                continue;
            }
            map[*cx] = Some(*cy);
            used[*cy] = true;
            queue.push_back((*cx, *cy));
        }
    }
    map
}

// ---- applying the delta ------------------------------------------------------------------------------

struct Merger<'a> {
    go: &'a G,
    gi: &'a G,
    /// `:out` table → `:in` table
    oi: &'a [Option<NodeId>],
    /// `:in` table → live table
    il: &'a [Option<NodeId>],
    /// `:out` tables copied into the live graph
    copies: HashMap<NodeId, NodeId>,
    applied: Applied,
}

impl Merger<'_> {
    /// Whether `:out` value `o` equals `:in` value `i` (a reference is equal
    /// when it names the matched table).
    fn same(&self, o: &V, i: &V) -> bool {
        match (o, i) {
            (V::Leaf(a), V::Leaf(b)) => a == b,
            (V::Node(a), V::Node(b)) => self.oi[*a] == Some(*b),
            _ => false,
        }
    }

    /// An `:out` value in the live graph: matched tables are the live ones,
    /// other tables are copied (once each).
    fn tr(&mut self, gl: &mut G, v: &V) -> V {
        let V::Node(x) = v else {
            return v.clone();
        };
        if let Some(l) = self.oi[*x].and_then(|i| self.il[i]) {
            return V::Node(l);
        }
        if let Some(&c) = self.copies.get(x) {
            return V::Node(c);
        }
        let c = gl.alloc();
        self.copies.insert(*x, c);
        let go = self.go;
        let src = &go.nodes[*x];
        let mut node = Node {
            head: src.head.clone(),
            mods: src.mods.clone(),
            ..Node::default()
        };
        for (k, v) in &src.strs {
            node.strs.insert(k.clone(), self.tr(gl, v));
        }
        for (k, v) in &src.nums {
            node.nums.insert(k.clone(), self.tr(gl, v));
        }
        for (k, v) in &src.keys {
            let kk = self.tr(gl, k);
            let vv = self.tr(gl, v);
            node.keys.push((kk, vv));
        }
        if let Some(m) = &src.meta {
            node.meta = Some(self.tr(gl, m));
        }
        gl.nodes[c] = node;
        V::Node(c)
    }

    fn apply_node(&mut self, gl: &mut G, o: NodeId, i: NodeId) {
        let (go, gi) = (self.go, self.gi);
        let (on, inn) = (&go.nodes[o], &gi.nodes[i]);
        let is_root = o == self.go.root;
        // what changed
        let mut sets: Vec<(MK, V)> = Vec::new();
        let mut removes: Vec<MK> = Vec::new();
        let mut keys: BTreeSet<MK> = on.path_members().into_iter().map(|(k, _)| k).collect();
        keys.extend(inn.path_members().into_iter().map(|(k, _)| k));
        for k in keys {
            if is_root && k == MK::S("locals".into()) {
                continue;
            }
            match (on.get(&k), inn.get(&k)) {
                (Some(ov), None) => sets.push((k, ov.clone())),
                (None, Some(_)) => removes.push(k),
                (Some(ov), Some(iv)) if !self.same(ov, iv) => sets.push((k, ov.clone())),
                _ => {}
            }
        }
        let head_changed = on.head != inn.head;
        let keys_changed = on.keys.len() != inn.keys.len()
            || on
                .keys
                .iter()
                .zip(&inn.keys)
                .any(|((ok, ov), (ik, iv))| !self.same(ok, ik) || !self.same(ov, iv));
        let mods_delta = mods_delta(on.mods.as_ref(), inn.mods.as_ref());
        if sets.is_empty()
            && removes.is_empty()
            && !head_changed
            && !keys_changed
            && mods_delta.is_none()
        {
            return;
        }
        self.applied.changed_tables += 1;
        let Some(l) = self.il[i] else {
            let n = sets.len() + removes.len();
            self.applied.notes.push(format!(
                "a table the callee changed ({n} keys{}) is not at its :in path in the caller's state",
                sets.first()
                    .map(|(k, _)| format!(", e.g. {k:?}"))
                    .unwrap_or_default()
            ));
            return;
        };
        let new_keys: Option<Vec<(V, V)>> = keys_changed.then(|| on.keys.clone());
        let new_head = head_changed.then(|| on.head.clone());
        let sets: Vec<(MK, V)> = sets
            .into_iter()
            .map(|(k, v)| {
                let v = self.tr(gl, &v);
                (k, v)
            })
            .collect();
        let new_keys = new_keys.map(|ks| {
            ks.iter()
                .map(|(k, v)| (self.tr(gl, k), self.tr(gl, v)))
                .collect::<Vec<_>>()
        });
        let ln = &mut gl.nodes[l];
        for (k, v) in sets {
            ln.set(&k, v);
            self.applied.edits += 1;
        }
        for k in removes {
            ln.remove(&k);
            self.applied.edits += 1;
        }
        if let Some(h) = new_head {
            for k in inn.head.keys() {
                if !h.contains_key(k) {
                    ln.head.remove(k);
                }
            }
            ln.head.extend(h);
            self.applied.edits += 1;
        }
        if let Some(ks) = new_keys {
            ln.keys = ks;
            self.applied.edits += 1;
        }
        if let Some(d) = mods_delta {
            self.applied.edits += apply_mods_delta(&mut ln.mods, d, on.mods.as_ref());
        }
    }
}

/// Mods removed and added by the callee.
enum ModsDelta {
    /// per encoded name: (removed, added)
    Db(BTreeMap<String, (Vec<J>, Vec<J>)>),
    List(Vec<J>, Vec<J>),
    /// different shapes: replace
    Replace,
}

fn mods_delta(o: Option<&Mods>, i: Option<&Mods>) -> Option<ModsDelta> {
    match (o, i) {
        (None, None) => None,
        (Some(Mods::Db(om)), Some(Mods::Db(im))) => {
            let empty = Vec::new();
            let mut out = BTreeMap::new();
            let names: BTreeSet<&String> = om.keys().chain(im.keys()).collect();
            for n in names {
                let (r, a) =
                    multiset_delta(om.get(n).unwrap_or(&empty), im.get(n).unwrap_or(&empty));
                if !r.is_empty() || !a.is_empty() {
                    out.insert(n.clone(), (r, a));
                }
            }
            (!out.is_empty()).then_some(ModsDelta::Db(out))
        }
        (Some(Mods::List(ol)), Some(Mods::List(il))) => {
            let (r, a) = multiset_delta(ol, il);
            (!r.is_empty() || !a.is_empty()).then_some(ModsDelta::List(r, a))
        }
        (o, i) => (o != i).then_some(ModsDelta::Replace),
    }
}

/// (`in` − `out`, `out` − `in`) as multisets of mod JSON; the added ones in
/// `out`'s order.
fn multiset_delta(o: &[J], i: &[J]) -> (Vec<J>, Vec<J>) {
    let mut avail: HashMap<String, usize> = HashMap::new();
    for m in i {
        *avail.entry(m.to_string()).or_default() += 1;
    }
    let mut added = Vec::new();
    for m in o {
        match avail.get_mut(&m.to_string()) {
            Some(c) if *c > 0 => *c -= 1,
            _ => added.push(m.clone()),
        }
    }
    let mut removed = Vec::new();
    for m in i {
        if let Some(c) = avail.get_mut(&m.to_string()) {
            if *c > 0 {
                *c -= 1;
                removed.push(m.clone());
            }
        }
    }
    (removed, added)
}

/// A mod's JSON without its provenance (`$at`; the live export has it only
/// for mods that came from a dump).
fn without_at(m: &J) -> J {
    match m {
        J::Object(o) => J::Object(
            o.iter()
                .filter(|(k, _)| k.as_str() != "$at")
                .map(|(k, v)| (k.clone(), v.clone()))
                .collect(),
        ),
        other => other.clone(),
    }
}

fn apply_list(list: &mut Vec<J>, removed: &[J], added: &[J]) -> usize {
    let mut edits = 0;
    for r in removed {
        let pos = list.iter().position(|m| m == r).or_else(|| {
            let rr = without_at(r);
            list.iter().position(|m| without_at(m) == rr)
        });
        if let Some(p) = pos {
            list.remove(p);
            edits += 1;
        }
    }
    for a in added {
        list.push(a.clone());
        edits += 1;
    }
    edits
}

fn apply_mods_delta(live: &mut Option<Mods>, d: ModsDelta, out: Option<&Mods>) -> usize {
    match (d, live.as_mut()) {
        (ModsDelta::Db(byname), Some(Mods::Db(lm))) => {
            let mut edits = 0;
            for (n, (r, a)) in byname {
                let list = lm.entry(n.clone()).or_default();
                edits += apply_list(list, &r, &a);
                if list.is_empty() {
                    lm.remove(&n);
                }
            }
            edits
        }
        (ModsDelta::List(r, a), Some(Mods::List(ll))) => apply_list(ll, &r, &a),
        _ => {
            *live = out.cloned();
            1
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn m(name: &str, v: i64) -> J {
        json!({"name": name, "type": "BASE", "value": v, "flags": 0, "keywordFlags": 0, "tags": [], "$at": "X.lua:1"})
    }

    #[test]
    fn encode_decode_is_identity() {
        let root = json!({
            "env": {"$id": 1, "player": {"output": {"Life": 10, "$$esc": 1, "$seq": [1, 2], "$num": {"5": true}},
                     "modDB": {"$store": "ModDB", "mods": {"Life": [m("Life", 1)]}, "parent": false},
                     "self": {"$ref": 1}, "list": [{"a": 1}, {"$ref": 2}]},
                    "shared": {"$id": 2, "x": 1}},
            "globalCache": {"cachedData": {"MAIN": {}}},
            "locals": {"env": {"$ref": 1}}
        });
        let g = G::decode(&root);
        assert_eq!(g.encode(), root);
    }

    #[test]
    fn delta_keeps_the_callers_state() {
        let din = json!({
            "env": {"player": {"output": {"Life": 10, "Gone": 1, "Same": 3},
                    "modDB": {"$store": "ModDB", "mods": {"Life": [m("Life", 1)]}}}},
            "globalCache": {}, "locals": {"x": 1}
        });
        let dout = json!({
            "env": {"player": {"output": {"Life": 10, "Same": 3, "TotalDPS": 5, "MainHand": {"Speed": 2}},
                    "modDB": {"$store": "ModDB", "mods": {"Life": [m("Life", 1), m("Life", 2)], "Mana": [m("Mana", 3)]}}}},
            "globalCache": {}, "locals": {"x": 2}
        });
        let live = json!({
            "env": {"player": {"output": {"Life": 99, "Gone": 1, "Same": 3, "Caller": 7},
                    "modDB": {"$store": "ModDB", "mods": {"Life": [m("Life", 1), m("Life", 9)]}}}},
            "globalCache": {}, "locals": {}
        });
        let (merged, applied) = merge(&live, &din, &dout);
        let want = json!({
            "env": {"player": {"output": {"Life": 99, "Same": 3, "Caller": 7, "TotalDPS": 5, "MainHand": {"Speed": 2}},
                    "modDB": {"$store": "ModDB", "mods": {"Life": [m("Life", 1), m("Life", 9), m("Life", 2)], "Mana": [m("Mana", 3)]}}}},
            "globalCache": {}, "locals": {}
        });
        assert_eq!(merged, want, "{applied:?}");
        assert!(applied.notes.is_empty());
    }

    #[test]
    fn references_translate_to_live_tables() {
        // the callee stores a reference to an existing table, and a new
        // shared table under two keys
        let din = json!({"env": {"a": {"v": 1}, "b": {}}, "globalCache": {}, "locals": {}});
        let dout = json!({"env": {"a": {"$id": 1, "v": 1}, "b": {"ra": {"$ref": 1}, "n1": {"$id": 2, "k": 1}, "n2": {"$ref": 2}}},
                          "globalCache": {}, "locals": {}});
        let live = json!({"env": {"a": {"v": 1, "caller": true}, "b": {"own": 1}}, "globalCache": {}, "locals": {}});
        let (merged, _) = merge(&live, &din, &dout);
        let want = json!({"env": {"a": {"$id": 1, "v": 1, "caller": true},
                                  "b": {"n1": {"$id": 2, "k": 1}, "n2": {"$ref": 2}, "own": 1, "ra": {"$ref": 1}}},
                          "globalCache": {}, "locals": {}});
        assert_eq!(merged, want);
    }
}
