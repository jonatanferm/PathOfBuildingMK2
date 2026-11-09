//! LuaJIT 2.1 table layout: integer-keyed Lua tables with LuaJIT's exact
//! `pairs()` order (calc doc D8, §3.4, §4.1 `ljtab.rs`), and the layout model
//! behind `#t` of the calc's dynamic tables (unit HYG1).
//!
//! A port of `lj_tab.c` (vendored `luajit-src 210.5.12+a4f56a4`, GC64): array
//! part, hash part with `hashkey` main positions (`hashrot` for numbers;
//! integer keys are stored as doubles in the hash part in both number modes,
//! so the layout is the same on x64 and ARM64), Brent's-variation collision
//! handling with a descending free pointer (`lj_tab_newkey`, including the
//! rechaining of moved nodes), dead keys (assigning nil keeps the node, and the
//! key stays in its chain), rehash sizing
//! (`rehashtab`/`countint`/`bestasize`/`lj_tab_resize`), `lj_tab_next` order
//! and `lj_tab_len`; plus table constructors (`lj_parse.c` `expr_table`,
//! `BC_TNEW`/`BC_TDUP`, [`table_ctor`]).
//!
//! [`LjTab`] is generic over the key ([`LjKey`]); [`LjIntMap`] is the
//! integer-keyed table of D8. Values are opaque to the layout. Every key goes
//! through the paths a Lua `t[k] = v` takes (`lj_meta_tset` →
//! `lj_tab_get`/`lj_tab_newkey`), so `t[k] = nil` on a missing key inserts a
//! dead node exactly as LuaJIT does.
//!
//! String keys: LuaJIT hashes a string by its `sid` (`hashstr`, `lj_tab.h`),
//! an id the interner hands out in creation order and reseeds from a PRNG
//! (`lj_str.c`), so where a string lands in the hash part differs between
//! processes and depends on every string the process interned before. No
//! deterministic model can match that; [`str_sid_standin`] is a fixed
//! stand-in. Tables whose keys are numbers and booleans only are modelled
//! exactly.

const NIL: u32 = u32::MAX;
const LJ_MAX_ABITS: usize = 28;
const LJ_MAX_ASIZE: u32 = (1 << (LJ_MAX_ABITS - 1)) + 1;
const LJ_MAX_HBITS: u32 = 26;

/// A key of a table modelled by [`LjTab`]: how `lj_tab.c` hashes and counts it.
pub trait LjKey: Copy + Eq {
    /// The key as an integer-valued Lua number, `None` for any other key.
    /// (Only `0 <= k < LJ_MAX_ASIZE` can live in the array part.)
    fn as_int(&self) -> Option<i64>;
    /// The number key `i`.
    fn from_int(i: i64) -> Self;
    /// `hashkey` (`lj_tab.c`) before `hashmask`.
    fn hash(&self) -> u32;
    /// A string key (`VKSTR` in `expr_table`).
    fn is_str(&self) -> bool {
        false
    }
}

impl LjKey for i64 {
    #[inline]
    fn as_int(&self) -> Option<i64> {
        Some(*self)
    }
    #[inline]
    fn from_int(i: i64) -> Self {
        i
    }
    #[inline]
    fn hash(&self) -> u32 {
        hashnum(*self)
    }
}

#[derive(Clone)]
struct Node<K, V> {
    /// `None` = nil key (a free node). A dead node keeps its key.
    key: Option<K>,
    val: Option<V>,
    next: u32,
}

impl<K, V> Node<K, V> {
    fn empty() -> Self {
        Self {
            key: None,
            val: None,
            next: NIL,
        }
    }
}

/// Where `lj_tab_set` returned a slot.
#[derive(Clone, Copy)]
enum Slot {
    Array(usize),
    Node(usize),
}

/// A Lua table as LuaJIT 2.1 lays it out (`GCtab`): array part, hash part,
/// free pointer. Values are opaque to the layout.
#[derive(Clone)]
pub struct LjTab<K, V> {
    /// `t->array`, `t->asize` slots (slot 0 is key 0).
    array: Vec<Option<V>>,
    /// `t->node`, `hmask + 1` nodes; empty = the shared `nilnode` (hmask 0).
    node: Vec<Node<K, V>>,
    /// `t->freetop` as an index into `node`.
    freetop: u32,
}

impl<K, V> Default for LjTab<K, V> {
    fn default() -> Self {
        Self {
            array: Vec::new(),
            node: Vec::new(),
            freetop: 0,
        }
    }
}

impl<K: LjKey + std::fmt::Debug, V: std::fmt::Debug + Clone> std::fmt::Debug for LjTab<K, V> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_map().entries(self.pairs()).finish()
    }
}

/* Hash constants. Tuned using a brute force search. */
const HASH_ROT1: u32 = 14;
const HASH_ROT2: u32 = 5;
const HASH_ROT3: u32 = 13;

/// `hashrot` (`lj_tab.h`): scramble the bits of numbers. `lj_tab.h` spells
/// it differently for x86/x64 ("prefer variant that compiles well for a
/// 2-operand CPU") and for other targets, but the two are the same function:
/// both compute `lo' = (lo ^ hi) - rol(hi, 14)`, `hi' = lo' ^ rol(hi, 19)`,
/// `hi' - rol(lo', 13)`. So `pairs()` order does not depend on the target.
#[inline]
fn hashrot(mut lo: u32, mut hi: u32) -> u32 {
    lo ^= hi;
    hi = hi.rotate_left(HASH_ROT1);
    lo = lo.wrapping_sub(hi);
    hi = hi.rotate_left(HASH_ROT2);
    hi ^= lo;
    hi = hi.wrapping_sub(lo.rotate_left(HASH_ROT3));
    hi
}

/// `hashnum` before masking: the number key `k` (a double in the hash part).
#[inline]
pub fn hashnum(k: i64) -> u32 {
    let u = (k as f64).to_bits();
    hashrot(u as u32, ((u >> 32) as u32) << 1)
}

/// `hashkey` of a boolean before masking (`hashmask(t, boolV(key))`).
#[inline]
pub fn hashbool(b: bool) -> u32 {
    u32::from(b)
}

/// Stand-in for a string's `sid` (`hashstr`): FNV-1a over the bytes. LuaJIT's
/// own `sid` is per process (module docs), so any fixed choice is as faithful
/// as another; this one is deterministic across runs and platforms.
#[inline]
pub fn str_sid_standin(s: &str) -> u32 {
    let mut h: u32 = 0x811c_9dc5;
    for &b in s.as_bytes() {
        h ^= u32::from(b);
        h = h.wrapping_mul(0x0100_0193);
    }
    h
}

/// `lj_fls`.
#[inline]
fn lj_fls(x: u32) -> u32 {
    31 - x.leading_zeros()
}

/// `hsize2hbits`.
#[inline]
fn hsize2hbits(s: u32) -> u32 {
    if s == 0 {
        0
    } else if s == 1 {
        1
    } else {
        1 + lj_fls(s - 1)
    }
}

/// `countint`: count an integer key that could go to the array part.
#[inline]
fn countint<K: LjKey>(key: K, bins: &mut [u32; LJ_MAX_ABITS]) -> u32 {
    match key.as_int() {
        Some(k) if k >= 0 && k < i64::from(LJ_MAX_ASIZE) => {
            let k = k as u32;
            bins[(if k > 2 { lj_fls(k - 1) } else { 0 }) as usize] += 1;
            1
        }
        _ => 0,
    }
}

/// `bestasize`.
fn bestasize(bins: &[u32; LJ_MAX_ABITS], narray: &mut u32) -> u32 {
    let nn = *narray;
    let mut na = 0u32;
    let mut sz = 0u32;
    let mut sum = 0u32;
    let mut b = 0usize;
    while 2 * nn > (1u32 << b) && sum != nn {
        if bins[b] > 0 {
            sum += bins[b];
            if 2 * sum > (1u32 << b) {
                sz = (2u32 << b) + 1;
                na = sum;
            }
        }
        b += 1;
    }
    *narray = sz;
    na
}

impl<K: LjKey, V: Clone> LjTab<K, V> {
    /// `{}`.
    pub fn new() -> Self {
        Self::default()
    }

    /// `lua_createtable(L, narray, nhash)` / `table.new(narray, nhash)`
    /// (`lj_tab_new_ah`).
    pub fn with_sizes(narray: u32, nhash: u32) -> Self {
        Self::with_raw(if narray > 0 { narray + 1 } else { 0 }, hsize2hbits(nhash))
    }

    /// `lj_tab_new(L, asize, hbits)`: `asize` array slots (slot 0 included)
    /// and `2^hbits` nodes (none for 0). `BC_TNEW` and template tables.
    pub fn with_raw(asize: u32, hbits: u32) -> Self {
        assert!(asize <= LJ_MAX_ASIZE, "table overflow");
        let mut t = Self::default();
        t.array = vec![None; asize as usize];
        if hbits != 0 {
            t.newhpart(hbits);
        }
        t
    }

    #[inline]
    fn hmask(&self) -> u32 {
        if self.node.is_empty() {
            0
        } else {
            (self.node.len() - 1) as u32
        }
    }

    /// `newhpart` + `clearhpart`.
    fn newhpart(&mut self, hbits: u32) {
        assert!(hbits <= LJ_MAX_HBITS, "table overflow");
        let hsize = 1usize << hbits;
        self.node = (0..hsize).map(|_| Node::empty()).collect();
        self.freetop = hsize as u32;
    }

    /// `hashkey(t, key)`: main position (hash part must exist).
    #[inline]
    fn mainpos(&self, k: K) -> usize {
        (k.hash() & self.hmask()) as usize
    }

    /// The array slot of `k`, if `k` is an integer key below `asize`.
    #[inline]
    fn inarray(&self, k: K) -> Option<usize> {
        match k.as_int() {
            Some(i) if i >= 0 && (i as u64) < self.array.len() as u64 => Some(i as usize),
            _ => None,
        }
    }

    /// `lj_tab_get` on the hash part (chain search, dead keys included).
    fn find_node(&self, k: K) -> Option<usize> {
        if self.node.is_empty() {
            return None;
        }
        let mut n = self.mainpos(k) as u32;
        loop {
            let node = &self.node[n as usize];
            if node.key == Some(k) {
                return Some(n as usize);
            }
            n = node.next;
            if n == NIL {
                return None;
            }
        }
    }

    /// Lua `t[k]`.
    pub fn get(&self, k: K) -> Option<V> {
        if let Some(i) = self.inarray(k) {
            return self.array[i].clone();
        }
        self.find_node(k).and_then(|n| self.node[n].val.clone())
    }

    /// Lua `t[k] ~= nil`.
    pub fn contains_key(&self, k: K) -> bool {
        if let Some(i) = self.inarray(k) {
            return self.array[i].is_some();
        }
        self.find_node(k)
            .is_some_and(|n| self.node[n].val.is_some())
    }

    /// Lua `t[k] = v`; `None` assigns nil (keeps the dead node, as LuaJIT does).
    pub fn set(&mut self, k: K, v: Option<V>) {
        match self.setslot(k) {
            Slot::Array(i) => self.array[i] = v,
            Slot::Node(n) => self.node[n].val = v,
        }
    }

    /// `lj_tab_set` (`lj_tab_setint` for integer keys).
    fn setslot(&mut self, k: K) -> Slot {
        if let Some(i) = self.inarray(k) {
            return Slot::Array(i);
        }
        if let Some(n) = self.find_node(k) {
            return Slot::Node(n);
        }
        self.newkey(k)
    }

    /// `lj_tab_newkey`: insert new key. Use Brent's variation to optimize the
    /// chain length.
    fn newkey(&mut self, k: K) -> Slot {
        if self.node.is_empty() {
            // Main position is the nilnode: no free node found.
            self.rehashtab(k); // Rehash table.
            return self.setslot(k); // Retry key insertion.
        }
        let mut n = self.mainpos(k);
        if self.node[n].val.is_some() {
            let mut freenode = self.freetop as usize;
            loop {
                if freenode == 0 {
                    // No free node found?
                    self.rehashtab(k); // Rehash table.
                    return self.setslot(k); // Retry key insertion.
                }
                freenode -= 1;
                if self.node[freenode].key.is_none() {
                    break;
                }
            }
            self.freetop = freenode as u32;
            let collide0 = self.mainpos(self.node[n].key.expect("live node has a key"));
            if collide0 != n {
                // Colliding node not the main node?
                let mut collide = collide0;
                while self.node[collide].next as usize != n {
                    // Find predecessor.
                    collide = self.node[collide].next as usize;
                }
                self.node[collide].next = freenode as u32; // Relink chain.
                                                           // Copy colliding node into free node and free main node.
                let moved_val = self.node[n].val.take();
                let moved_key = self.node[n].key;
                let moved_next = self.node[n].next;
                self.node[freenode].val = moved_val;
                self.node[freenode].key = moved_key;
                self.node[freenode].next = moved_next;
                self.node[n].next = NIL;
                // (n->val was set to nil by the take() above.)
                // Rechain pseudo-resurrected string keys with colliding hashes.
                let mut fnode = freenode;
                while self.node[fnode].next != NIL {
                    let nn = self.node[fnode].next as usize;
                    if self.node[nn].val.is_some()
                        && self.mainpos(self.node[nn].key.expect("key")) == n
                    {
                        self.node[fnode].next = self.node[nn].next;
                        self.node[nn].next = self.node[n].next;
                        self.node[n].next = nn as u32;
                        // Rechaining a resurrected string key creates a new dilemma:
                        // Another string key may have originally been resurrected via
                        // _any_ of the previous nodes as a chain anchor. Including
                        // a node that had to be moved, which makes them unreachable.
                        // It's not feasible to check for all previous nodes, so rechain
                        // any string key that's currently in a non-main positions.
                        loop {
                            let nn = self.node[fnode].next;
                            if nn == NIL {
                                break;
                            }
                            let nn = nn as usize;
                            if self.node[nn].val.is_some() {
                                let mn = self.mainpos(self.node[nn].key.expect("key"));
                                if mn != fnode && mn != nn {
                                    self.node[fnode].next = self.node[nn].next;
                                    self.node[nn].next = self.node[mn].next;
                                    self.node[mn].next = nn as u32;
                                } else {
                                    fnode = nn;
                                }
                            } else {
                                fnode = nn;
                            }
                        }
                        break;
                    }
                    fnode = nn;
                }
            } else {
                // Otherwise use free node.
                self.node[freenode].next = self.node[n].next; // Insert into chain.
                self.node[n].next = freenode as u32;
                n = freenode;
            }
        }
        self.node[n].key = Some(k);
        debug_assert!(self.node[n].val.is_none(), "new hash slot is not empty");
        Slot::Node(n)
    }

    /// `countarray`.
    fn countarray(&self, bins: &mut [u32; LJ_MAX_ABITS]) -> u32 {
        let asize = self.array.len() as u32;
        if asize == 0 {
            return 0;
        }
        let mut na = 0u32;
        let mut i = 0u32;
        for b in 0..LJ_MAX_ABITS {
            let mut top = 2u32 << b;
            if top >= asize {
                top = asize - 1;
                if i > top {
                    break;
                }
            }
            let mut n = 0u32;
            while i <= top {
                if self.array[i as usize].is_some() {
                    n += 1;
                }
                i += 1;
            }
            bins[b] += n;
            na += n;
        }
        na
    }

    /// `counthash`.
    fn counthash(&self, bins: &mut [u32; LJ_MAX_ABITS], narray: &mut u32) -> u32 {
        let mut total = 0u32;
        let mut na = 0u32;
        for n in &self.node {
            if n.val.is_some() {
                na += countint(n.key.expect("key"), bins);
                total += 1;
            }
        }
        *narray += na;
        total
    }

    /// `rehashtab`.
    fn rehashtab(&mut self, ek: K) {
        let mut bins = [0u32; LJ_MAX_ABITS];
        let mut asize = self.countarray(&mut bins);
        let mut total = 1 + asize;
        total += self.counthash(&mut bins, &mut asize);
        asize += countint(ek, &mut bins);
        let na = bestasize(&bins, &mut asize);
        total -= na;
        self.resize(asize, hsize2hbits(total));
    }

    /// `lj_tab_resize`: resize a table to fit the new array/hash part sizes.
    fn resize(&mut self, asize: u32, hbits: u32) {
        let oldnode = std::mem::take(&mut self.node);
        let oldasize = self.array.len() as u32;
        if asize > oldasize {
            // Array part grows?
            assert!(asize <= LJ_MAX_ASIZE, "table overflow");
            self.array.resize(asize as usize, None);
        }
        // Create new (empty) hash part.
        if hbits != 0 {
            self.newhpart(hbits);
        } else {
            self.node = Vec::new();
            self.freetop = 0;
        }
        if asize < oldasize {
            // Array part shrinks?
            let tail = self.array.split_off(asize as usize);
            for (j, v) in tail.into_iter().enumerate() {
                // Reinsert old array values.
                if v.is_some() {
                    let i = K::from_int(i64::from(asize) + j as i64);
                    // lj_tab_setinth
                    let slot = match self.find_node(i) {
                        Some(n) => Slot::Node(n),
                        None => self.newkey(i),
                    };
                    match slot {
                        Slot::Array(a) => self.array[a] = v,
                        Slot::Node(n) => self.node[n].val = v,
                    }
                }
            }
        }
        // Reinsert pairs from old hash part.
        for n in oldnode {
            if n.val.is_some() {
                self.set(n.key.expect("key"), n.val);
            }
        }
    }

    /// `lj_tab_reasize`: resize the array part to `nasize + 1` slots, keeping
    /// the hash part's size (`expr_table` on a template table).
    pub fn reasize(&mut self, nasize: u32) {
        let hmask = self.hmask();
        self.resize(nasize + 1, if hmask > 0 { lj_fls(hmask) + 1 } else { 0 });
    }

    /// `#t` (`lj_tab_len`).
    pub fn len(&self) -> usize {
        let asize = self.array.len();
        let mut hi = asize;
        if hi != 0 {
            hi -= 1;
        }
        // In a growing array the last array element is very likely nil.
        if hi > 0 && self.array[hi].is_none() {
            // Binary search to find a non-nil to nil transition in the array.
            let mut lo = 0usize;
            while hi - lo > 1 {
                let mid = (lo + hi) >> 1;
                if self.array[mid].is_none() {
                    hi = mid;
                } else {
                    lo = mid;
                }
            }
            return lo;
        }
        // Without a hash part, there's an implicit nil after the last element.
        if self.hmask() != 0 {
            self.tab_len_slow(hi)
        } else {
            hi
        }
    }

    /// `#t == 0`.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// `tab_len_slow`: mixed array/hash lookups.
    fn tab_len_slow(&self, hi0: usize) -> usize {
        let present = |k: usize| self.contains_key(K::from_int(k as i64));
        let mut lo = hi0;
        let mut hi = hi0 + 1;
        // Widening search for an upper bound.
        while present(hi) {
            lo = hi;
            hi += hi;
            if hi > 0x7fff_ffff - 2 {
                // Punt and do a linear search.
                lo = 1;
                while present(lo) {
                    lo += 1;
                }
                return lo - 1;
            }
        }
        // Binary search to find a non-nil to nil transition.
        while hi - lo > 1 {
            let mid = (lo + hi) >> 1;
            if present(mid) {
                lo = mid;
            } else {
                hi = mid;
            }
        }
        lo
    }

    /// `pairs(t)` in `lj_tab_next` order: array part, then hash nodes in
    /// index order. A snapshot of the keys with their current values.
    pub fn pairs(&self) -> Vec<(K, V)> {
        let mut out = Vec::new();
        for (i, v) in self.array.iter().enumerate() {
            if let Some(v) = v {
                out.push((K::from_int(i as i64), v.clone()));
            }
        }
        for n in &self.node {
            if let Some(v) = &n.val {
                out.push((n.key.expect("live node has a key"), v.clone()));
            }
        }
        out
    }

    /// The keys of `pairs(t)`, in order.
    pub fn keys(&self) -> Vec<K> {
        let mut out = Vec::new();
        for (i, v) in self.array.iter().enumerate() {
            if v.is_some() {
                out.push(K::from_int(i as i64));
            }
        }
        for n in &self.node {
            if n.val.is_some() {
                out.push(n.key.expect("key"));
            }
        }
        out
    }

    /// Common.lua `copyTable(t, true)`: a new `{}` filled in `pairs` order.
    pub fn copyTable(&self) -> Self {
        let mut out = Self::new();
        for (k, v) in self.pairs() {
            out.set(k, Some(v));
        }
        out
    }

    /// Common.lua `wipeTable(t)`: assign nil to every key in `pairs` order
    /// (the nodes stay, dead, as in LuaJIT).
    pub fn wipe(&mut self) {
        for k in self.keys() {
            self.set(k, None);
        }
    }

    /// `(asize, hsize)`: the array part size (including slot 0) and the
    /// number of hash nodes. For tests and diagnostics.
    pub fn sizes(&self) -> (usize, usize) {
        (self.array.len(), self.node.len())
    }

    /// The same layout with every value mapped (`None` = nil, keeping the key
    /// as a dead node).
    fn map_values<W>(self, mut f: impl FnMut(V) -> Option<W>) -> LjTab<K, W> {
        LjTab {
            array: self.array.into_iter().map(|v| v.and_then(&mut f)).collect(),
            node: self
                .node
                .into_iter()
                .map(|n| Node {
                    key: n.key,
                    val: n.val.and_then(&mut f),
                    next: n.next,
                })
                .collect(),
            freetop: self.freetop,
        }
    }
}

// ---------------------------------------------------------------------------
// Table constructors: lj_parse.c `expr_table`, BC_TNEW, BC_TDUP (lj_tab_dup).
// ---------------------------------------------------------------------------

/// The key of one field of a Lua table constructor `{ ... }`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CtorKey<K> {
    /// A positional item `v`: the key is the next array index.
    Item,
    /// `name = v` (a string key).
    Named(K),
    /// `[k] = v`; `konst`: `k` is a constant expression (`expr_isk`: a
    /// boolean, number or string literal, after constant folding).
    Bracket { key: K, konst: bool },
}

/// One field of a Lua table constructor, as `expr_table` classifies it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CtorField<K> {
    pub key: CtorKey<K>,
    /// The value is a constant (`expr_isk_nojump`: nil, false, true, a number
    /// or a string literal, after constant folding).
    pub konst: bool,
    /// The value is nil (for a constant: the literal `nil`).
    pub nil: bool,
}

/// What a constructor leaves before its run-time stores.
pub struct Ctor<K> {
    /// The table `BC_TNEW` creates, or the exact copy of the template table
    /// that `BC_TDUP` makes (`lj_tab_dup`), with the template's constant
    /// values present (the values are `()`: only presence is modelled).
    pub table: LjTab<K, ()>,
    /// Per field, in source order: its key, and `true` when its value is
    /// stored at run time (`TSETB`/`TSETV`/`TSETS` after the table exists, in
    /// field order), `false` when the value came with the template.
    pub fields: Vec<(K, bool)>,
}

#[derive(Clone)]
enum TemplateVal {
    Konst,
    /// `settabV(fs->L, v, t)`: "Preserve key with table itself as value".
    Dummy,
}

/// `expr_table` (lj_parse.c:1723-1829) for a constructor without a
/// multiple-results call as its last item (`vcall`/`BC_TSETM` is not
/// modelled), then `BC_TNEW` (`lj_tab_new`; vm_*.dasc turns asize `0x7ff`
/// into `0x801`) or `BC_TDUP` (`lj_tab_dup`: an exact copy, free pointer
/// included). The caller then performs the run-time stores.
pub fn table_ctor<K: LjKey>(fields: &[CtorField<K>]) -> Ctor<K> {
    let mut t: Option<LjTab<K, TemplateVal>> = None;
    let mut needarr = false;
    let mut narr: u32 = 1; // First array index.
    let mut nhash: u32 = 0; // Number of hash entries.
    let mut out = Vec::with_capacity(fields.len());
    for f in fields {
        let (key, key_konst) = match f.key {
            CtorKey::Bracket { key, konst } => {
                if konst && key.as_int() == Some(0) {
                    needarr = true;
                } else {
                    nhash += 1;
                }
                (key, konst)
            }
            CtorKey::Named(key) => {
                nhash += 1;
                (key, true)
            }
            CtorKey::Item => {
                let key = K::from_int(i64::from(narr));
                narr += 1;
                needarr = true;
                (key, true)
            }
        };
        // expr_isk(&key) && key.k != VKNIL && (key.k == VKSTR || expr_isk_nojump(&val))
        let runtime = if key_konst && (key.is_str() || f.konst) {
            // Create template table on demand.
            let tt = t.get_or_insert_with(|| {
                LjTab::with_raw(if needarr { narr } else { 0 }, hsize2hbits(nhash))
            });
            if f.konst {
                // Add const key/value to template table.
                tt.set(key, (!f.nil).then_some(TemplateVal::Konst));
                false
            } else {
                // Otherwise create dummy string key (avoids lj_tab_newkey).
                tt.set(key, Some(TemplateVal::Dummy));
                true
            }
        } else {
            true
        };
        out.push((key, runtime));
    }
    let table = match t {
        None => {
            // Construct TNEW RD: hhhhhaaaaaaaaaaa.
            let narr = if !needarr {
                0
            } else if narr < 3 {
                3
            } else if narr > 0x7ff {
                0x7ff
            } else {
                narr
            };
            // BC_TNEW: "Turn 0x7ff into 0x801."
            let asize = if narr == 0x7ff { 0x801 } else { narr };
            LjTab::with_raw(asize, hsize2hbits(nhash))
        }
        Some(mut tt) => {
            if needarr && (tt.array.len() as u32) < narr {
                tt.reasize(narr - 1);
            }
            // Fix value for dummy keys in template table: turn value into nil.
            tt.map_values(|v| match v {
                TemplateVal::Konst => Some(()),
                TemplateVal::Dummy => None,
            })
        }
    };
    Ctor { table, fields: out }
}

// ---------------------------------------------------------------------------
// LjIntMap: the integer-keyed table of D8 (frozen API, calc doc §4.1).
// ---------------------------------------------------------------------------

/// Integer-keyed Lua table whose `pairs()` order is LuaJIT 2.1's (D8). Values
/// are opaque to the layout.
#[derive(Clone)]
pub struct LjIntMap<V>(LjTab<i64, V>);

impl<V> Default for LjIntMap<V> {
    fn default() -> Self {
        Self(LjTab::default())
    }
}

impl<V: std::fmt::Debug + Clone> std::fmt::Debug for LjIntMap<V> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(f)
    }
}

impl<V: Clone> LjIntMap<V> {
    /// `{}`.
    pub fn new() -> Self {
        Self::default()
    }

    /// `lua_createtable(L, narray, nhash)` / `table.new(narray, nhash)`
    /// (`lj_tab_new_ah`).
    pub fn with_sizes(narray: u32, nhash: u32) -> Self {
        Self(LjTab::with_sizes(narray, nhash))
    }

    /// Lua `t[k]`.
    pub fn get(&self, k: i64) -> Option<V> {
        self.0.get(k)
    }

    /// Lua `t[k] ~= nil`.
    pub fn contains_key(&self, k: i64) -> bool {
        self.0.contains_key(k)
    }

    /// Lua `t[k] = v`; `None` assigns nil (keeps the dead node, as LuaJIT does).
    pub fn set(&mut self, k: i64, v: Option<V>) {
        self.0.set(k, v);
    }

    /// `#t` (`lj_tab_len`).
    pub fn len(&self) -> usize {
        self.0.len()
    }

    /// `#t == 0`.
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// `pairs(t)` in `lj_tab_next` order: array part, then hash nodes in
    /// index order. A snapshot of the keys with their current values.
    pub fn pairs(&self) -> Vec<(i64, V)> {
        self.0.pairs()
    }

    /// The keys of `pairs(t)`, in order.
    pub fn keys(&self) -> Vec<i64> {
        self.0.keys()
    }

    /// Common.lua `copyTable(t, true)`: a new `{}` filled in `pairs` order.
    pub fn copyTable(&self) -> Self {
        Self(self.0.copyTable())
    }

    /// Common.lua `wipeTable(t)`: assign nil to every key in `pairs` order
    /// (the nodes stay, dead, as in LuaJIT).
    pub fn wipe(&mut self) {
        self.0.wipe();
    }

    /// `(asize, hsize)`: the array part size (including slot 0) and the
    /// number of hash nodes. For tests and diagnostics.
    pub fn sizes(&self) -> (usize, usize) {
        self.0.sizes()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// calc doc §3.4: measured on the vendored LuaJIT (the aarch64 oracle).
    /// The two `hashrot` spellings in `lj_tab.h` are the same function.
    #[test]
    fn hashrot_variants_agree() {
        let generic = |mut lo: u32, mut hi: u32| {
            lo ^= hi;
            lo = lo.wrapping_sub(hi.rotate_left(HASH_ROT1));
            hi = lo ^ hi.rotate_left(HASH_ROT1 + HASH_ROT2);
            hi.wrapping_sub(lo.rotate_left(HASH_ROT3))
        };
        let mut x: u64 = 0x9e37_79b9_7f4a_7c15;
        for _ in 0..100_000 {
            x ^= x << 13;
            x ^= x >> 7;
            x ^= x << 17;
            assert_eq!(
                hashrot(x as u32, (x >> 32) as u32),
                generic(x as u32, (x >> 32) as u32)
            );
        }
    }

    #[test]
    fn doc_example_order() {
        let mut t = LjIntMap::new();
        for k in [52031, 1031, 26725, 55485, 33545, 6, 7, 1, 2, 3, 99999, 4184] {
            t.set(k, Some(true));
        }
        assert_eq!(
            t.keys(),
            vec![1, 2, 3, 6, 7, 26725, 4184, 55485, 99999, 1031, 52031, 33545]
        );
    }

    #[test]
    fn basics() {
        let mut t = LjIntMap::new();
        for k in 1..=10 {
            t.set(k, Some(k * 10));
        }
        assert_eq!(t.len(), 10);
        assert_eq!(t.get(3), Some(30));
        t.set(3, None);
        assert_eq!(t.get(3), None);
        assert!(!t.contains_key(3));
        t.set(-5, Some(1));
        t.set(70000, Some(2));
        assert_eq!(t.get(-5), Some(1));
        let c = t.copyTable();
        assert_eq!(c.keys(), t.keys());
        t.wipe();
        assert!(t.pairs().is_empty());
        let w = LjIntMap::<u8>::with_sizes(4, 3);
        assert_eq!(w.sizes(), (5, 4));
    }

    /// HYG1: the borders the unit names (checked with `luajit -e` under
    /// `jit.off()`): sequential stores then a hole give 1; a constructor of
    /// variables (`BC_TNEW`, asize 4) gives 3; a constructor of constants
    /// (template grown by rehashes to asize 5) gives 1.
    #[test]
    fn hyg1_borders() {
        let mut t: LjTab<i64, ()> = LjTab::new();
        for k in 1..=3 {
            t.set(k, Some(()));
        }
        t.set(2, None);
        assert_eq!(t.len(), 1);

        let item = |konst| CtorField {
            key: CtorKey::Item,
            konst,
            nil: false,
        };
        let mut c = table_ctor::<i64>(&[item(false), item(false), item(false)]);
        assert!(c.fields.iter().all(|&(_, rt)| rt));
        for &(k, _) in &c.fields {
            c.table.set(k, Some(()));
        }
        assert_eq!(c.table.sizes(), (4, 0));
        c.table.set(2, None);
        assert_eq!(c.table.len(), 3);

        let mut c = table_ctor::<i64>(&[item(true), item(true), item(true)]);
        assert!(c.fields.iter().all(|&(_, rt)| !rt));
        assert_eq!(c.table.sizes(), (5, 0));
        c.table.set(2, None);
        assert_eq!(c.table.len(), 1);
    }
}
