//! Dynamic Lua values and tables (calc doc §5.1, D3).
//!
//! `output`, `breakdown`, `skillData`, `skillFlags`, `conditions`,
//! `multipliers`, `configInput` and every other table whose keys PoB computes
//! is a [`Table`]: an `Rc` handle (clone = new alias, D2) to an array part plus
//! an insertion-ordered hash part (§3.4: string-keyed `pairs` order follows
//! insertion; PoB's own order is random per process). Accessors take one short
//! borrow each and never hand out references (§3.3).
#![allow(non_snake_case)]

use std::any::Any;
use std::cell::RefCell;
use std::fmt;
use std::rc::Rc;

use pob_gamedata::{ModRef, Value, ValueTable};
use pob_lua::{lua_error, CheapClone, Name};

/// Non-key strings (breakdown lines) are not interned.
pub type LStr = Rc<str>;

/// A Lua value as the calc sees it.
#[derive(Clone, Default)]
pub enum Val {
    #[default]
    Nil,
    Bool(bool),
    Num(f64),
    Str(LStr),
    /// A mutable calc table.
    Table(Table),
    /// An immutable table from data or a mod (writes raise LuaError). Scalars
    /// never appear here: [`Val::from_value`] maps them to the native variants.
    Data(Value),
    Mod(ModRef),
    /// A calc struct stored in a dynamic table (`ActiveSkillRef` in SkillDPS
    /// rows, items in `env.flasks`, …).
    Obj(Rc<dyn Any>),
}

impl fmt::Debug for Val {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Val::Nil => f.write_str("nil"),
            Val::Bool(b) => write!(f, "{b}"),
            Val::Num(n) => f.write_str(&pob_lua::tostring_num(*n)),
            Val::Str(s) => write!(f, "{s:?}"),
            Val::Table(t) => write!(f, "table: {:p}", Rc::as_ptr(&t.0)),
            Val::Data(v) => write!(f, "data: {v:?}"),
            Val::Mod(m) => write!(f, "mod: {}", m.name),
            Val::Obj(_) => f.write_str("userdata"),
        }
    }
}

impl Val {
    #[inline]
    pub fn truthy(&self) -> bool {
        !matches!(self, Val::Nil | Val::Bool(false))
    }

    #[inline]
    pub fn is_nil(&self) -> bool {
        matches!(self, Val::Nil)
    }

    /// Arithmetic operand: a number, or a numeric string (Lua coercion);
    /// anything else raises "attempt to perform arithmetic on a … value".
    pub fn num(&self) -> f64 {
        match self {
            Val::Num(n) => *n,
            Val::Str(s) => pob_lua::tonumber(s)
                .unwrap_or_else(|| lua_error!("attempt to perform arithmetic on a string value")),
            other => lua_error!(
                "attempt to perform arithmetic on a {} value",
                other.lua_type()
            ),
        }
    }

    /// `Some` only for a number.
    #[inline]
    pub fn opt_num(&self) -> Option<f64> {
        match self {
            Val::Num(n) => Some(*n),
            _ => None,
        }
    }

    /// `x or d`: nil/false → `d`, a number → it, anything else raises (the
    /// result would be used as a number).
    pub fn num_or(&self, d: f64) -> f64 {
        match self {
            Val::Nil | Val::Bool(false) => d,
            Val::Num(n) => *n,
            other => other.num(),
        }
    }

    pub fn str(&self) -> Option<LStr> {
        match self {
            Val::Str(s) => Some(s.clone()),
            _ => None,
        }
    }

    pub fn table(&self) -> Option<Table> {
        match self {
            Val::Table(t) => Some(t.clone()),
            _ => None,
        }
    }

    pub fn obj<T: 'static>(&self) -> Option<Rc<T>> {
        match self {
            Val::Obj(o) => o.clone().downcast::<T>().ok(),
            _ => None,
        }
    }

    /// Lua `type(v)`.
    pub fn lua_type(&self) -> &'static str {
        match self {
            Val::Nil => "nil",
            Val::Bool(_) => "boolean",
            Val::Num(_) => "number",
            Val::Str(_) => "string",
            Val::Table(_) | Val::Data(_) | Val::Mod(_) => "table",
            Val::Obj(_) => "userdata",
        }
    }

    /// Indexing a value: tables (with `__index`), data tables and mods; nil
    /// and other kinds raise "attempt to index …".
    pub fn get(&self, k: impl Into<Key>) -> Val {
        let k = k.into();
        match self {
            Val::Table(t) => t.get(k),
            Val::Data(v) => data_index(v, k),
            Val::Mod(m) => mod_index(m, k),
            Val::Str(_) => Val::Nil, // string methods are not table data
            other => lua_error!("attempt to index a {} value", other.lua_type()),
        }
    }

    /// An immutable data value as a calc value (scalars become native).
    pub fn from_value(v: &Value) -> Val {
        match v {
            Value::Nil => Val::Nil,
            Value::Bool(b) => Val::Bool(*b),
            Value::Num(n) => Val::Num(*n),
            Value::Str(s) => Val::Str(LStr::from(&**s)),
            Value::Mod(m) => Val::Mod(m.clone()),
            other => Val::Data(other.clone()),
        }
    }

    /// Lua `==`: tables compare by identity, strings and numbers by value.
    pub fn raw_eq(&self, o: &Val) -> bool {
        match (self, o) {
            (Val::Nil, Val::Nil) => true,
            (Val::Bool(a), Val::Bool(b)) => a == b,
            (Val::Num(a), Val::Num(b)) => a == b,
            (Val::Str(a), Val::Str(b)) => a == b,
            (Val::Table(a), Val::Table(b)) => a.ptr_eq(b),
            (Val::Mod(a), Val::Mod(b)) => std::sync::Arc::ptr_eq(a, b),
            (Val::Obj(a), Val::Obj(b)) => Rc::ptr_eq(a, b),
            (Val::Data(a), Val::Data(b)) => match (a, b) {
                (Value::Table(x), Value::Table(y)) => std::sync::Arc::ptr_eq(x, y),
                (Value::JewelFunc(x), Value::JewelFunc(y)) => std::sync::Arc::ptr_eq(x, y),
                _ => false,
            },
            _ => false,
        }
    }
}

fn data_index(v: &Value, k: Key) -> Val {
    match v {
        Value::Table(t) => match k {
            Key::S(n) => Val::from_value(t.get(n)),
            Key::I(i) if i >= 1 => Val::from_value(t.index(i as usize)),
            _ => Val::Nil,
        },
        Value::Mod(m) => mod_index(m, k),
        Value::JewelFunc(j) => match k {
            Key::S(n) if n.as_str() == "type" => j
                .r#type
                .map_or(Val::Nil, |t| Val::Str(LStr::from(t.as_str()))),
            Key::S(n) if n.as_str() == "radius" => j
                .radius
                .map_or(Val::Nil, |t| Val::Str(LStr::from(t.as_str()))),
            _ => Val::Nil,
        },
        _ => lua_error!("attempt to index a {} value", v.lua_type()),
    }
}

fn mod_index(m: &ModRef, k: Key) -> Val {
    match k {
        Key::S(n) => match n.as_str() {
            "name" => Val::Str(LStr::from(m.name.as_str())),
            "type" => match m.r#type {
                pob_gamedata::ModType::Nil => Val::Nil,
                t => Val::Str(LStr::from(t.as_str())),
            },
            "value" => Val::from_value(&m.value),
            "flags" => match m.extra_get(pob_lua::n!("flags")) {
                Some(raw) => Val::from_value(raw),
                None => Val::Num(f64::from(m.flags)),
            },
            "keywordFlags" => Val::Num(f64::from(m.keywordFlags)),
            "source" => m
                .source
                .as_ref()
                .map_or(Val::Nil, |s| Val::Str(LStr::from(&**s))),
            _ => m.extra_get(n).map_or(Val::Nil, Val::from_value),
        },
        Key::I(i) if i >= 1 => match m.tags.get(i as usize - 1) {
            Some(t) => Val::Data(Value::Table(std::sync::Arc::new(
                pob_gamedata::modifier::tag_to_value_table(t),
            ))),
            None => Val::Nil,
        },
        _ => Val::Nil,
    }
}

impl From<f64> for Val {
    fn from(n: f64) -> Self {
        Val::Num(n)
    }
}
impl From<i32> for Val {
    fn from(n: i32) -> Self {
        Val::Num(f64::from(n))
    }
}
impl From<bool> for Val {
    fn from(b: bool) -> Self {
        Val::Bool(b)
    }
}
impl From<&str> for Val {
    fn from(s: &str) -> Self {
        Val::Str(LStr::from(s))
    }
}
impl From<String> for Val {
    fn from(s: String) -> Self {
        Val::Str(LStr::from(s))
    }
}
impl From<LStr> for Val {
    fn from(s: LStr) -> Self {
        Val::Str(s)
    }
}
impl From<Name> for Val {
    fn from(s: Name) -> Self {
        Val::Str(LStr::from(s.as_str()))
    }
}
impl From<Table> for Val {
    fn from(t: Table) -> Self {
        Val::Table(t)
    }
}
impl From<Option<f64>> for Val {
    fn from(n: Option<f64>) -> Self {
        n.map_or(Val::Nil, Val::Num)
    }
}
impl From<Option<Table>> for Val {
    fn from(t: Option<Table>) -> Self {
        t.map_or(Val::Nil, Val::Table)
    }
}
impl From<ModRef> for Val {
    fn from(m: ModRef) -> Self {
        Val::Mod(m)
    }
}
impl From<Value> for Val {
    fn from(v: Value) -> Self {
        Val::from_value(&v)
    }
}

/// `a or b` on values (b evaluated only when needed).
pub fn lor(a: Val, b: impl FnOnce() -> Val) -> Val {
    if a.truthy() {
        a
    } else {
        b()
    }
}

/// `a and b` on values.
pub fn land(a: Val, b: impl FnOnce() -> Val) -> Val {
    if a.truthy() {
        b()
    } else {
        a
    }
}

/// A table key. Float keys are integral (others raise, §5.1); table-identity
/// keys use `PtrSet`/`PtrMap` instead.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum Key {
    S(Name),
    I(i64),
    B(bool),
}

impl From<Name> for Key {
    fn from(n: Name) -> Self {
        Key::S(n)
    }
}
impl From<&str> for Key {
    fn from(s: &str) -> Self {
        Key::S(Name::from(s))
    }
}
impl From<i64> for Key {
    fn from(i: i64) -> Self {
        Key::I(i)
    }
}
impl From<i32> for Key {
    fn from(i: i32) -> Self {
        Key::I(i64::from(i))
    }
}
impl From<usize> for Key {
    fn from(i: usize) -> Self {
        Key::I(i as i64)
    }
}
impl From<bool> for Key {
    fn from(b: bool) -> Self {
        Key::B(b)
    }
}
impl From<f64> for Key {
    fn from(x: f64) -> Self {
        if x == x.trunc() && x.is_finite() && x.abs() < 9.007_199_254_740_992e15 {
            Key::I(x as i64)
        } else if x.is_nan() {
            lua_error!("table index is NaN")
        } else {
            lua_error!(
                "non-integral number keys are not supported by Table ({})",
                pob_lua::tostring_num(x)
            )
        }
    }
}

impl Key {
    /// The key as a Lua value (for `pairs` loops that use the key).
    pub fn to_val(self) -> Val {
        match self {
            Key::S(n) => Val::from(n),
            Key::I(i) => Val::Num(i as f64),
            Key::B(b) => Val::Bool(b),
        }
    }

    pub fn name(self) -> Option<Name> {
        match self {
            Key::S(n) => Some(n),
            _ => None,
        }
    }
}

/// `setmetatable(t, { __index = … })` (U0 audited every in-scope site, §8).
#[derive(Clone)]
pub enum IndexMeta {
    /// `{ __index = parent }`: CalcActiveSkill.lua:482/487.
    Table(Table),
    /// `__index = function(_, k) return a[k] or b[k] end`: CalcOffence.lua:4422/4695/5000.
    OrChain(smallvec::SmallVec<[Table; 2]>),
}

/// The contents of a [`Table`].
#[derive(Default)]
pub struct TableData {
    /// Keys `1..=len`, no trailing nil.
    arr: Vec<Val>,
    /// Insertion order; assigning nil leaves a tombstone (Lua's dead key).
    hash: indexmap::IndexMap<Key, Val, ahash::RandomState>,
    meta: Option<IndexMeta>,
}

/// A mutable Lua table. `Clone` = a new alias (`Rc`).
#[derive(Clone, Default)]
pub struct Table(Rc<RefCell<TableData>>);

impl CheapClone for Table {}

impl fmt::Debug for Table {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let d = self.0.borrow();
        write!(f, "{{")?;
        for (i, v) in d.arr.iter().enumerate() {
            write!(f, "[{}]={v:?}, ", i + 1)?;
        }
        for (k, v) in &d.hash {
            if !v.is_nil() {
                write!(f, "{k:?}={v:?}, ")?;
            }
        }
        write!(f, "}}")
    }
}

/// Lua `#t` on the array part plus trailing integer keys in the hash part.
fn migrate_from_hash(d: &mut TableData) {
    loop {
        let next = Key::I(d.arr.len() as i64 + 1);
        match d.hash.get(&next) {
            Some(v) if !v.is_nil() => {
                let v = d.hash.shift_remove(&next).expect("present");
                d.arr.push(v);
            }
            _ => break,
        }
    }
}

impl Table {
    pub fn new() -> Self {
        Table::default()
    }

    pub fn with_meta(meta: IndexMeta) -> Self {
        let t = Table::new();
        t.0.borrow_mut().meta = Some(meta);
        t
    }

    /// `setmetatable(t, meta)` on an existing table.
    pub fn set_meta(&self, meta: Option<IndexMeta>) {
        self.0.borrow_mut().meta = meta;
    }

    // ---- reads ----

    /// `rawget(t, k)`.
    pub fn rawget(&self, k: impl Into<Key>) -> Val {
        let k = k.into();
        let d = self.0.borrow();
        if let Key::I(i) = k {
            if i >= 1 && (i as usize) <= d.arr.len() {
                return d.arr[i as usize - 1].clone();
            }
        }
        d.hash.get(&k).cloned().unwrap_or(Val::Nil)
    }

    /// `t[k]`, including `__index`.
    pub fn get(&self, k: impl Into<Key>) -> Val {
        let k = k.into();
        let v = self.rawget(k);
        if !v.is_nil() {
            return v;
        }
        let meta = self.0.borrow().meta.clone();
        match meta {
            None => Val::Nil,
            Some(IndexMeta::Table(p)) => p.get(k),
            Some(IndexMeta::OrChain(l)) => {
                let mut r = Val::Nil;
                for t in &l {
                    r = t.get(k);
                    if r.truthy() {
                        return r;
                    }
                }
                r
            }
        }
    }

    /// `if t[k] then`.
    pub fn truthy(&self, k: impl Into<Key>) -> bool {
        self.get(k).truthy()
    }

    /// `t[k] or 0`.
    pub fn num0(&self, k: impl Into<Key>) -> f64 {
        self.get(k).num_or(0.0)
    }

    /// `t[k] or d`.
    pub fn num_or(&self, k: impl Into<Key>, d: f64) -> f64 {
        self.get(k).num_or(d)
    }

    /// `t[k]` in arithmetic (LuaError on nil).
    pub fn num(&self, k: impl Into<Key>) -> f64 {
        self.get(k).num()
    }

    pub fn opt_num(&self, k: impl Into<Key>) -> Option<f64> {
        self.get(k).opt_num()
    }

    pub fn str(&self, k: impl Into<Key>) -> Option<LStr> {
        self.get(k).str()
    }

    pub fn table(&self, k: impl Into<Key>) -> Option<Table> {
        self.get(k).table()
    }

    pub fn obj<T: 'static>(&self, k: impl Into<Key>) -> Option<Rc<T>> {
        self.get(k).obj()
    }

    // ---- writes ----

    /// `t[k] = v` (Nil deletes, leaving a tombstone in the hash part).
    pub fn set(&self, k: impl Into<Key>, v: impl Into<Val>) {
        let k = k.into();
        let v = v.into();
        let mut d = self.0.borrow_mut();
        if let Key::I(i) = k {
            let len = d.arr.len();
            if i >= 1 && (i as usize) <= len {
                d.arr[i as usize - 1] = v;
                if i as usize == len {
                    // keep "no trailing nil": the border moves down
                    while d.arr.last().is_some_and(Val::is_nil) {
                        d.arr.pop();
                    }
                }
                return;
            }
            if i as usize == len + 1 && i >= 1 {
                if v.is_nil() {
                    if let Some(slot) = d.hash.get_mut(&k) {
                        *slot = Val::Nil;
                    }
                    return;
                }
                d.hash.shift_remove(&k);
                d.arr.push(v);
                migrate_from_hash(&mut d);
                return;
            }
        }
        match d.hash.get_mut(&k) {
            Some(slot) => *slot = v,
            None => {
                if !v.is_nil() {
                    d.hash.insert(k, v);
                }
            }
        }
    }

    /// `t[k] = t[k] or {}`; returns the table.
    pub fn get_or_new_table(&self, k: impl Into<Key>) -> Table {
        let k = k.into();
        match self.get(k) {
            Val::Table(t) => t,
            v if v.truthy() => lua_error!("get_or_new_table: t[{k:?}] is a {}", v.lua_type()),
            _ => {
                let t = Table::new();
                self.set(k, t.clone());
                t
            }
        }
    }

    /// `t[k] = (t[k] or 0) + d`.
    pub fn add(&self, k: impl Into<Key>, d: f64) {
        let k = k.into();
        let v = self.get(k).num_or(0.0) + d;
        self.set(k, v);
    }

    // ---- array part ----

    /// `#t`.
    pub fn len(&self) -> usize {
        self.0.borrow().arr.len()
    }

    pub fn is_empty(&self) -> bool {
        let d = self.0.borrow();
        d.arr.is_empty() && d.hash.values().all(Val::is_nil)
    }

    /// `t_insert(t, v)`.
    pub fn push(&self, v: impl Into<Val>) {
        let n = self.len();
        self.set(Key::I(n as i64 + 1), v);
    }

    /// `t_insert(t, pos, v)`, exactly as LuaJIT 2.1 `lib_table.c` `table_insert`:
    /// no bounds check. With `i = #t + 1`, every `t[k] = t[k-1]` for
    /// `k = i, i-1, ..., pos+1` (raw), then `t[pos] = v`. So `pos > #t + 1` just
    /// sets `t[pos]` (a hole; `#t` is unchanged) and `pos <= 0` shifts `t[pos..=#t]`
    /// up by one through the hash part.
    pub fn insert(&self, pos: i64, v: impl Into<Val>) {
        let v = v.into();
        let mut i = self.len() as i64 + 1;
        while i > pos {
            let src = self.rawget(Key::I(i - 1));
            self.set(Key::I(i), src);
            i -= 1;
        }
        self.set(Key::I(pos), v);
    }

    /// `t_remove(t)` / `t_remove(t, pos)`, exactly as LuaJIT 2.1 `lib_table.c`
    /// `table_remove`: it acts only when `pos` is nil (and `#t ~= 0`) or
    /// `1 <= pos <= #t`; any other `pos` returns nothing and raises no error.
    pub fn remove(&self, pos: Option<i64>) -> Val {
        let len = self.len() as i64;
        match pos {
            None => {
                if len != 0 {
                    let old = self.get(Key::I(len));
                    self.set(Key::I(len), Val::Nil);
                    old
                } else {
                    Val::Nil
                }
            }
            Some(pos) if pos >= 1 && pos <= len => {
                let old = self.get(Key::I(pos));
                for i in pos + 1..=len {
                    let v = self.get(Key::I(i));
                    self.set(Key::I(i - 1), v);
                }
                self.set(Key::I(len), Val::Nil);
                old
            }
            Some(_) => Val::Nil,
        }
    }

    /// `pairs(t)`: a key snapshot (array part, then hash part in insertion
    /// order); each value is read live and keys that are nil when reached are
    /// skipped.
    pub fn pairs(&self) -> TablePairs {
        let d = self.0.borrow();
        let mut keys: Vec<Key> = (1..=d.arr.len() as i64).map(Key::I).collect();
        keys.extend(d.hash.iter().filter(|(_, v)| !v.is_nil()).map(|(k, _)| *k));
        TablePairs {
            t: self.clone(),
            keys,
            i: 0,
        }
    }

    /// `ipairs(t)` snapshot of the array part (callers that need Lua's live
    /// read use `for_ipairs!`).
    pub fn ipairs(&self) -> Vec<(usize, Val)> {
        let mut out = Vec::new();
        let mut i = 1;
        loop {
            let v = self.rawget(Key::I(i as i64));
            if v.is_nil() {
                break;
            }
            out.push((i, v));
            i += 1;
        }
        out
    }

    /// `wipeTable(t)` (keeps the metatable).
    /// @pob Modules/Common.lua:549-557 h=8970b4d5
    pub fn wipe(&self) {
        let mut d = self.0.borrow_mut();
        d.arr.clear();
        d.hash.clear();
    }

    /// Common.lua `copyTable(tbl, noRecurse)`. It does NOT keep the metatable
    /// (CCR-0011: calc doc §3.2 says it does; at the pin only `copyTableSafe` with
    /// `preserveMeta` does, Common.lua:494-496). Data values (`Val::Data`,
    /// immutable) are shared, which equals a deep copy.
    /// @pob Modules/Common.lua:477-487 h=2e6837b7
    pub fn copyTable(&self, noRecurse: bool) -> Table {
        // L478
        let out = Table::new();
        // L479
        for (k, v) in self.pairs() {
            // L480
            match v {
                Val::Table(t) if !noRecurse => out.set(k, t.copyTable(false)),
                v => out.set(k, v),
            }
        }
        out
    }

    pub fn ptr_eq(&self, o: &Table) -> bool {
        Rc::ptr_eq(&self.0, &o.0)
    }

    pub fn meta(&self) -> Option<IndexMeta> {
        self.0.borrow().meta.clone()
    }

    /// Identity of the table (for `PtrMap`-style keys and debugging).
    pub fn as_ptr(&self) -> *const () {
        Rc::as_ptr(&self.0).cast()
    }

    /// A data table converted into a fresh calc table (`copyTable` of data).
    pub fn from_value_table(t: &ValueTable) -> Table {
        let out = Table::new();
        for v in &t.array {
            out.push(Val::from_value(v));
        }
        for (k, v) in &t.fields {
            out.set(*k, Val::from_value(v));
        }
        out
    }
}

/// `for_ipairs!((i, v) in t => …)` over a table: reads `t[i]` (raw) live each
/// step and stops at the first nil, as Lua's `ipairs` does.
impl pob_lua::ipairs::IpairsGet for Table {
    type Item = Val;
    fn ipairs_get(&self, i1: usize) -> Option<Val> {
        let v = self.rawget(Key::I(i1 as i64));
        (!v.is_nil()).then_some(v)
    }
}

/// See [`Table::pairs`].
pub struct TablePairs {
    t: Table,
    keys: Vec<Key>,
    i: usize,
}

impl Iterator for TablePairs {
    type Item = (Key, Val);
    fn next(&mut self) -> Option<(Key, Val)> {
        while self.i < self.keys.len() {
            let k = self.keys[self.i];
            self.i += 1;
            let v = self.t.rawget(k);
            if !v.is_nil() {
                return Some((k, v));
            }
        }
        None
    }
}

/// `tbl![ "a", "b" ]`, `tbl!{ key = v, … }`, `tbl!{ "a"; key = v }`,
/// `tbl!{ [k] => v, … }` (computed keys).
#[macro_export]
macro_rules! tbl {
    () => { $crate::lua_table::Table::new() };
    ($($k:ident = $v:expr),+ $(,)?) => {{
        let t = $crate::lua_table::Table::new();
        $( t.set({ static N: ::std::sync::OnceLock<::pob_lua::Name> = ::std::sync::OnceLock::new(); *N.get_or_init(|| ::pob_lua::Name::from(stringify!($k))) }, $v); )+
        t
    }};
    ($([$k:expr] => $v:expr),+ $(,)?) => {{
        let t = $crate::lua_table::Table::new();
        $( t.set($k, $v); )+
        t
    }};
    ($($a:expr),+ ; $($k:ident = $v:expr),* $(,)?) => {{
        let t = $crate::lua_table::Table::new();
        $( t.push($a); )+
        $( t.set({ static N: ::std::sync::OnceLock<::pob_lua::Name> = ::std::sync::OnceLock::new(); *N.get_or_init(|| ::pob_lua::Name::from(stringify!($k))) }, $v); )*
        t
    }};
    ($($a:expr),+ $(,)?) => {{
        let t = $crate::lua_table::Table::new();
        $( t.push($a); )+
        t
    }};
}

/// `actor.output`, `output.MainHand`, minion output: same type and keys.
pub type Output = Table;

#[cfg(test)]
mod tests {
    use super::*;
    use pob_lua::n;

    #[test]
    fn array_and_hash() {
        let t = Table::new();
        t.set(3, 30.0);
        assert_eq!(t.len(), 0);
        t.set(1, 10.0);
        t.set(2, 20.0);
        assert_eq!(t.len(), 3);
        t.set(n!("Life"), 5.0);
        assert_eq!(t.num0(n!("Life")), 5.0);
        assert_eq!(t.num0(n!("Mana")), 0.0);
        t.set(n!("Life"), Val::Nil);
        assert!(!t.truthy(n!("Life")));
        t.set(n!("Life"), 6.0);
        let keys: Vec<Key> = t.pairs().map(|(k, _)| k).collect();
        assert_eq!(
            keys,
            vec![Key::I(1), Key::I(2), Key::I(3), Key::S(n!("Life"))]
        );
        t.insert(1, 5.0);
        assert_eq!(t.len(), 4);
        assert_eq!(t.remove(Some(1)).num(), 5.0);
        assert_eq!(t.remove(None).num(), 30.0);
        assert_eq!(t.len(), 2);
        let c = t.copyTable(true);
        assert!(!c.ptr_eq(&t));
        assert_eq!(c.num(1), 10.0);
    }

    /// LuaJIT 2.1 `lib_table.c` semantics (no Lua 5.3 bounds errors). Expected
    /// values were taken from `luajit -e` on the same calls.
    #[test]
    fn insert_remove_luajit() {
        fn dump(t: &Table) -> (Vec<(i64, f64)>, usize) {
            let kv = (-2..=8)
                .filter_map(|k| t.rawget(Key::I(k)).opt_num().map(|v| (k, v)))
                .collect();
            (kv, t.len())
        }
        let t123 = || tbl![1.0, 2.0, 3.0];
        let t = t123();
        t.insert(6, 9.0);
        assert_eq!(dump(&t), (vec![(1, 1.0), (2, 2.0), (3, 3.0), (6, 9.0)], 3));
        let t = t123();
        assert!(t.remove(Some(7)).is_nil());
        assert_eq!(dump(&t), (vec![(1, 1.0), (2, 2.0), (3, 3.0)], 3));
        let t = t123();
        assert!(t.remove(Some(0)).is_nil());
        assert!(t.remove(Some(4)).is_nil());
        assert_eq!(dump(&t), (vec![(1, 1.0), (2, 2.0), (3, 3.0)], 3));
        let t = t123();
        t.insert(0, 9.0);
        assert_eq!(dump(&t), (vec![(0, 9.0), (2, 1.0), (3, 2.0), (4, 3.0)], 4));
        let t = t123();
        t.insert(-1, 9.0);
        assert_eq!(dump(&t), (vec![(-1, 9.0), (2, 1.0), (3, 2.0), (4, 3.0)], 4));
        let t = t123();
        t.insert(2, 9.0);
        assert_eq!(dump(&t), (vec![(1, 1.0), (2, 9.0), (3, 2.0), (4, 3.0)], 4));
        let t = t123();
        t.insert(4, 9.0);
        assert_eq!(dump(&t), (vec![(1, 1.0), (2, 2.0), (3, 3.0), (4, 9.0)], 4));
        let t = Table::new();
        assert!(t.remove(None).is_nil());
        assert_eq!(dump(&t), (vec![], 0));
        let t = t123();
        assert_eq!(t.remove(Some(1)).num(), 1.0);
        assert_eq!(dump(&t), (vec![(1, 2.0), (2, 3.0)], 2));
    }

    #[test]
    fn index_meta() {
        let a = tbl! { X = 1.0 };
        let b = tbl! { Y = 2.0 };
        let c = Table::with_meta(IndexMeta::OrChain(smallvec::smallvec![
            a.clone(),
            b.clone()
        ]));
        assert_eq!(c.num(n!("X")), 1.0);
        assert_eq!(c.num(n!("Y")), 2.0);
        let d = Table::with_meta(IndexMeta::Table(a));
        assert_eq!(d.num(n!("X")), 1.0);
        assert!(d.rawget(n!("X")).is_nil());
        let e = tbl!["a", "b"; k = true];
        assert_eq!(e.len(), 2);
        assert!(e.truthy(n!("k")));
    }

    #[test]
    fn errors() {
        let r = pob_lua::error::catch(|| Val::Nil.num());
        assert!(r.is_err());
        let r = pob_lua::error::catch(|| Val::Nil.get(n!("x")));
        assert!(r.is_err());
    }
}
