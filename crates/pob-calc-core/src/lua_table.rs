//! Dynamic Lua values and tables (calc doc §5.1, D3).
//!
//! `output`, `breakdown`, `skillData`, `skillFlags`, `conditions`,
//! `multipliers`, `configInput` and every other table whose keys PoB computes
//! is a [`Table`]: an `Rc` handle (clone = new alias, D2) to an array part plus
//! an insertion-ordered hash part (§3.4: string-keyed `pairs` order follows
//! insertion; PoB's own order is random per process). Accessors take one short
//! borrow each and never hand out references (§3.3).
//!
//! `#t` (HYG1): next to its storage every table keeps a model of the layout
//! LuaJIT would give it (`pob_lua::ljtab::LjTab`: array part, hash part, dead
//! keys, rehash sizing), fed by every store in the order Lua makes them, and
//! [`Table::len`] is `lj_tab_len` on that model. A border is therefore the one
//! LuaJIT picks, holes included: `t[1]=1 t[2]=2 t[3]=3 t[2]=nil` gives 1, a
//! constructor `{a, b, c}` then `t[2]=nil` gives 3 (`BC_TNEW` asize 4), and a
//! constructor of constants `{1, 2, 3}` then `t[2]=nil` gives 1 (the template
//! grew by rehashes). Constructors go through [`Table::from_ctor`] / `tbl!`.
//! Exact for tables keyed by numbers and booleans; string keys use a
//! stand-in for LuaJIT's per-process string ids (`ljtab` module docs), so for
//! a table mixing string keys with holed integer keys LuaJIT itself can give
//! different borders in different processes. `len()` is always a valid border.
//!
//! Number keys (FUP1, CCR-0102): any number but NaN is a key, normalised as
//! `lj_tab.c` does: an integer value is [`Key::I`] (`t[2.0]` is `t[2]`,
//! `t[-0.0]` is `t[0]`), any other float is [`Key::F`], a hash key (`hashnum`
//! of its bits in the layout model; `pairs` order is insertion order with the
//! other hash keys, calc doc D8). Storing at NaN raises "table index is NaN";
//! reading at NaN is nil ([`Key::try_num`]).
#![allow(non_snake_case)]

use std::any::Any;
use std::cell::RefCell;
use std::fmt;
use std::rc::Rc;

use pob_gamedata::{ModRef, Value, ValueTable};
use pob_lua::ljtab::{self, CtorKey, LjKey, LjTab};
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
    #[track_caller]
    pub fn num(&self) -> f64 {
        match self {
            Val::Num(n) => *n,
            // `match`, not `unwrap_or_else`: a closure would hide `#[track_caller]`'s caller.
            Val::Str(s) => match pob_lua::tonumber(s) {
                Some(n) => n,
                None => lua_error!("attempt to perform arithmetic on a string value"),
            },
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
    #[track_caller]
    pub fn get(&self, k: impl Into<Key>) -> Val {
        let k = k.into();
        match self {
            Val::Table(t) => t.get(k),
            Val::Data(v) => data_index(v, k),
            Val::Mod(m) => mod_index(m, k),
            Val::Str(_) => Val::Nil, // string methods are not table data
            other => lua_error!(
                "attempt to index a {} value{}",
                other.lua_type(),
                key_note(k)
            ),
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

#[track_caller]
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
        _ => lua_error!("attempt to index a {} value{}", v.lua_type(), key_note(k)),
    }
}

/// ` (key 'name')` / ` (key 3)`: the key a failed index looked up, appended to "attempt to
/// index a … value" (Rust-only; PoB's message names the indexed expression, which we do not
/// know here) so a crash report points at the read that failed.
fn key_note(k: Key) -> String {
    match k {
        Key::S(n) => format!(" (key '{}')", n.as_str()),
        Key::I(i) => format!(" (key {i})"),
        Key::B(b) => format!(" (key {b})"),
        Key::F(f) => format!(" (key {})", f.get()),
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

/// A table key (FUP1: float keys as in `lj_tab.c`). A number key is
/// normalised the way LuaJIT normalises it: a float with an exact integer value
/// is the integer key ([`Key::I`]; `-0.0` is `0`), any other float is a hash
/// key of its own ([`Key::F`]); a NaN key raises "table index is NaN" on a
/// store ([`Key::from`]) and reads nil ([`Key::try_num`]). Table-identity keys
/// use `PtrSet`/`PtrMap` instead.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum Key {
    S(Name),
    I(i64),
    B(bool),
    /// A number key that is not an `i64` integer (calc doc D8: always a hash
    /// key, `pairs` in insertion order with the other hash keys).
    F(FKey),
}

/// A float table key: a finite non-integral float, an integral float outside
/// the `i64` range, or `+-inf`. Never NaN and never `+-0` (those are integral),
/// so bit equality is Lua's number equality.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct FKey(u64);

impl FKey {
    /// The key's number.
    #[inline]
    pub fn get(self) -> f64 {
        f64::from_bits(self.0)
    }
}

impl fmt::Debug for FKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&pob_lua::tostring_num(self.get()))
    }
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
/// A number as a key to store at (`t[x] = v`): NaN raises "table index is NaN"
/// (`lj_tab_newkey`, `LJ_ERR_NANIDX`).
impl From<f64> for Key {
    fn from(x: f64) -> Self {
        Key::try_num(x).unwrap_or_else(|| lua_error!("table index is NaN"))
    }
}

/// `-2^63`, the smallest `i64` (exact as a double); `2^63` is the first
/// double above `i64::MAX`.
const I64_MIN_F: f64 = i64::MIN as f64;

impl Key {
    /// A number as a key to read at (`t[x]`): `None` for NaN, which no table
    /// holds (`lj_tab_get` finds nothing, so the read is nil). Otherwise the
    /// key LuaJIT normalises `x` to (`lj_tab_set`/`lj_tab_get`: an exact
    /// integer value is the integer key, `-0.0 == 0`; any other float is a
    /// number key in the hash part).
    #[inline]
    pub fn try_num(x: f64) -> Option<Key> {
        if x.is_nan() {
            None
        } else if x == x.trunc() && (I64_MIN_F..-I64_MIN_F).contains(&x) {
            // exact: x is integral and inside the i64 range (`-0.0 as i64` is 0)
            Some(Key::I(x as i64))
        } else {
            Some(Key::F(FKey(x.to_bits())))
        }
    }

    /// The key as a Lua value (for `pairs` loops that use the key).
    pub fn to_val(self) -> Val {
        match self {
            Key::S(n) => Val::from(n),
            Key::I(i) => Val::Num(i as f64),
            Key::B(b) => Val::Bool(b),
            Key::F(f) => Val::Num(f.get()),
        }
    }

    /// The key's number, for an integer or float key.
    #[inline]
    pub fn as_num(self) -> Option<f64> {
        match self {
            Key::I(i) => Some(i as f64),
            Key::F(f) => Some(f.get()),
            _ => None,
        }
    }

    pub fn name(self) -> Option<Name> {
        match self {
            Key::S(n) => Some(n),
            _ => None,
        }
    }
}

/// How LuaJIT's `lj_tab.c` hashes and counts a key (the `#t` layout model).
impl LjKey for Key {
    #[inline]
    fn as_int(&self) -> Option<i64> {
        match *self {
            Key::I(i) => Some(i),
            _ => None,
        }
    }
    #[inline]
    fn from_int(i: i64) -> Self {
        Key::I(i)
    }
    #[inline]
    fn hash(&self) -> u32 {
        match *self {
            Key::I(i) => ljtab::hashnum(i),
            Key::B(b) => ljtab::hashbool(b),
            Key::S(n) => ljtab::str_sid_standin(n.as_str()),
            Key::F(f) => ljtab::hashnum_f64(f.get()),
        }
    }
    #[inline]
    fn is_str(&self) -> bool {
        matches!(self, Key::S(_))
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
    /// Storage for keys `1..=arr.len()` (a dense prefix; may hold nils, no
    /// trailing nil). Not the border: see `layout`.
    arr: Vec<Val>,
    /// Insertion order; assigning nil leaves a tombstone (Lua's dead key).
    hash: indexmap::IndexMap<Key, Val, ahash::RandomState>,
    meta: Option<IndexMeta>,
    /// LuaJIT's layout of this table (presence only), for `#t` (module docs).
    layout: LjTab<Key, ()>,
}

/// One field of a Lua table constructor `{ ... }` ([`Table::from_ctor`],
/// `tbl!`): its key, its value, and whether the value is a Lua constant
/// (`nil`, `true`, `false`, a number or a string literal; LuaJIT keeps those
/// in a template table, `lj_parse.c` `expr_table`).
pub struct TableField {
    pub key: CtorKey<Key>,
    pub konst: bool,
    pub val: Val,
}

impl TableField {
    /// A positional item `v`.
    pub fn item(konst: bool, val: impl Into<Val>) -> Self {
        Self {
            key: CtorKey::Item,
            konst,
            val: val.into(),
        }
    }
    /// `name = v`.
    pub fn named(name: Name, konst: bool, val: impl Into<Val>) -> Self {
        Self {
            key: CtorKey::Named(Key::S(name)),
            konst,
            val: val.into(),
        }
    }
    /// `[k] = v`; `key_konst`: `k` is a constant (a literal).
    pub fn bracket(key: impl Into<Key>, key_konst: bool, konst: bool, val: impl Into<Val>) -> Self {
        Self {
            key: CtorKey::Bracket {
                key: key.into(),
                konst: key_konst,
            },
            konst,
            val: val.into(),
        }
    }
}

/// `tbl!` helper: is the Rust source text of a constructor key or value (from
/// `stringify!`) a Lua constant, i.e. a literal: `true`, `false`, `Val::Nil`,
/// a number literal (optionally negated), a string literal or `n!("…")`?
/// LuaJIT also folds constant arithmetic (`1 + 2`); write such a value as its
/// folded literal.
#[doc(hidden)]
pub const fn __is_lua_const(s: &str) -> bool {
    const fn eq(b: &[u8], i: usize, j: usize, w: &[u8]) -> bool {
        if j - i != w.len() {
            return false;
        }
        let mut p = 0;
        while p < w.len() {
            if b[i + p] != w[p] {
                return false;
            }
            p += 1;
        }
        true
    }
    const fn str_lit(b: &[u8], i: usize, j: usize) -> bool {
        if j - i < 2 || b[i] != b'"' || b[j - 1] != b'"' {
            return false;
        }
        let mut p = i + 1;
        while p < j - 1 {
            if b[p] == b'\\' {
                p += 2;
                continue;
            }
            if b[p] == b'"' {
                return false;
            }
            p += 1;
        }
        p == j - 1
    }
    /// Is the Rust number literal `b[i..j]` (starting with a digit) zero? Its
    /// mantissa digits (before an exponent or a type suffix) are all `0`.
    const fn num_lit_is_zero(b: &[u8], i: usize, j: usize) -> bool {
        let hex = j - i > 2 && b[i] == b'0' && (b[i + 1] == b'x' || b[i + 1] == b'X');
        let mut p = if hex { i + 2 } else { i };
        while p < j {
            let c = b[p];
            if c == b'_' || c == b'.' || c == b'0' {
                p += 1;
                continue;
            }
            let digit = if hex {
                c.is_ascii_hexdigit()
            } else {
                c.is_ascii_digit()
            };
            // a non-zero digit: non-zero; anything else ends the mantissa
            return !digit;
        }
        true
    }
    let b = s.as_bytes();
    let (mut i, mut j) = (0, b.len());
    while i < j && b[i] == b' ' {
        i += 1;
    }
    while j > i && b[j - 1] == b' ' {
        j -= 1;
    }
    if i == j {
        return false;
    }
    if eq(b, i, j, b"true")
        || eq(b, i, j, b"false")
        || eq(b, i, j, b"Val::Nil")
        || eq(b, i, j, b"Val :: Nil")
    {
        return true;
    }
    if b[i] == b'"' {
        return str_lit(b, i, j);
    }
    // n!("…") / n! ("…")
    if j - i > 4 && b[i] == b'n' && b[i + 1] == b'!' && b[j - 1] == b')' {
        let mut p = i + 2;
        while p < j && b[p] == b' ' {
            p += 1;
        }
        if p < j && b[p] == b'(' {
            let (mut a, mut z) = (p + 1, j - 1);
            while a < z && b[a] == b' ' {
                a += 1;
            }
            while z > a && b[z - 1] == b' ' {
                z -= 1;
            }
            return str_lit(b, a, z);
        }
        return false;
    }
    // number literal, optionally negated
    let mut p = i;
    let negated = b[p] == b'-';
    if negated {
        p += 1;
        while p < j && b[p] == b' ' {
            p += 1;
        }
    }
    if p >= j || !b[p].is_ascii_digit() {
        return false;
    }
    // FUP1: LuaJIT does not fold the negation of a zero (`lj_parse.c`
    // `bcemit_unop`: `expr_isnumk(e) && !expr_numiszero(e)`), so `-0.0` is a
    // run-time value (`[-0.0] = v` is a run-time key).
    if negated && num_lit_is_zero(b, p, j) {
        return false;
    }
    let mut prev = 0u8;
    while p < j {
        let c = b[p];
        let ok = c.is_ascii_alphanumeric()
            || c == b'_'
            || c == b'.'
            || ((c == b'+' || c == b'-') && (prev == b'e' || prev == b'E'));
        if !ok {
            return false;
        }
        prev = c;
        p += 1;
    }
    true
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

/// Storage: move integer keys that now extend the dense prefix out of the hash
/// part.
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
    #[track_caller]
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
        // LuaJIT layout: every store, nil included (a missing key gets a dead node).
        d.layout.set(k, (!v.is_nil()).then_some(()));
        Self::store(&mut d, k, v);
    }

    /// The storage half of `t[k] = v` (the layout is the caller's).
    fn store(d: &mut TableData, k: Key, v: Val) {
        if let Key::I(i) = k {
            let len = d.arr.len();
            if i >= 1 && (i as usize) <= len {
                d.arr[i as usize - 1] = v;
                if i as usize == len {
                    // storage keeps "no trailing nil" (not the border: `layout`)
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
                migrate_from_hash(d);
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
    #[track_caller]
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
        // lj_tab_len on the LuaJIT layout model (module docs).
        self.0.borrow().layout.len()
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
        // L553-555: `tbl[k] = nil` for every key; LuaJIT keeps the sizes and
        // the (now dead) nodes.
        d.layout.wipe();
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
        // LuaJIT layout: Lua fills `out` in LuaJIT's `pairs` order of `tbl`
        // (lj_tab_next), not in this table's insertion order.
        let layout = self.0.borrow().layout.copyTable();
        out.0.borrow_mut().layout = layout;
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

    /// A table constructor `{ ... }` (`tbl!`): LuaJIT's `expr_table` builds a
    /// template table from the constant fields and `BC_TDUP` copies it, or
    /// `BC_TNEW` presizes an empty table; the other fields are stored at run
    /// time in field order (`pob_lua::ljtab::table_ctor`). The values in the
    /// template are in place before any run-time store, as in Lua (so with a
    /// duplicate key the run-time field wins: `{a = x, a = 1}` gives `x`).
    ///
    /// Storage, which gives `pairs` order (calc doc §3.4: insertion order),
    /// is FIELD order: every field is stored in turn, as the U0 `tbl!` did,
    /// and LuaJIT's parser likewise gives a run-time `name = v` field its
    /// template slot in field order (`lj_parse.c` `expr_table`: a `settabV`
    /// dummy, cleared by `fixt`). So `tbl!{ a = x, b = 1.0, c = x }` iterates
    /// `a, b, c`. Only a key whose last field is a constant one after a
    /// run-time one is then set again, to the run-time value (it keeps its
    /// slot unless the constant was nil). The layout (`#t`) follows LuaJIT's
    /// own order: template, then the run-time stores.
    pub fn from_ctor(fields: Vec<TableField>) -> Table {
        let shape: Vec<ljtab::CtorField<Key>> = fields
            .iter()
            .map(|f| ljtab::CtorField {
                key: f.key,
                konst: f.konst,
                nil: f.val.is_nil(),
            })
            .collect();
        let ctor = ljtab::table_ctor(&shape);
        let t = Table::new();
        let mut d = t.0.borrow_mut();
        d.layout = ctor.table;
        // Layout: TSETB/TSETV/TSETS on the TDUP/TNEW table, in field order.
        for (f, &(k, runtime)) in fields.iter().zip(&ctor.fields) {
            if runtime {
                d.layout.set(k, (!f.val.is_nil()).then_some(()));
            }
        }
        // Lua's values: a run-time field beats every constant field of its
        // key (the template is in place first), field order otherwise.
        let any_runtime = ctor.fields.iter().any(|&(_, rt)| rt);
        let any_konst = ctor.fields.iter().any(|&(_, rt)| !rt);
        let mut fix: Vec<(Key, Val)> = Vec::new();
        if any_runtime && any_konst {
            let keys = &ctor.fields;
            for (i, &(k, rt)) in keys.iter().enumerate() {
                let last = !keys[i + 1..].iter().any(|&(k2, _)| k2 == k);
                if rt || !last {
                    continue;
                }
                if let Some(j) = (0..i).rev().find(|&j| keys[j] == (k, true)) {
                    fix.push((k, fields[j].val.clone()));
                }
            }
        }
        // Storage: every field in turn.
        for (f, (k, _)) in fields.into_iter().zip(ctor.fields) {
            Self::store(&mut d, k, f.val);
        }
        for (k, v) in fix {
            Self::store(&mut d, k, v);
        }
        drop(d);
        t
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

/// A Lua table constructor: `tbl![ "a", "b" ]`, `tbl!{ key = v, … }`,
/// `tbl!{ "a"; key = v }`, `tbl!{ [k] => v, … }` (computed keys), `tbl!()`.
/// Built by [`Table::from_ctor`] with LuaJIT's layout: a value (or a `[k]`
/// key) written as a literal (`1.0`, `-2`, `"s"`, `n!("s")`, `true`,
/// `false`, `Val::Nil`) is a Lua constant and goes into the template table;
/// anything else is stored at run time (`__is_lua_const`). That split shapes
/// `#t` (and which of two fields for one key wins), not `pairs`: its order is
/// field order (§3.4, [`Table::from_ctor`]), and items are keyed by
/// position (`tbl![a, Val::Nil, c]` is `{[1] = a, [3] = c}`).
#[macro_export]
macro_rules! tbl {
    () => { $crate::lua_table::Table::new() };
    ($($k:ident = $v:expr),+ $(,)?) => {
        $crate::lua_table::Table::from_ctor(::std::vec![
            $( $crate::lua_table::TableField::named(
                { static N: ::std::sync::OnceLock<::pob_lua::Name> = ::std::sync::OnceLock::new(); *N.get_or_init(|| ::pob_lua::Name::from(stringify!($k))) },
                { const K: bool = $crate::lua_table::__is_lua_const(stringify!($v)); K },
                $v,
            ), )+
        ])
    };
    ($([$k:expr] => $v:expr),+ $(,)?) => {
        $crate::lua_table::Table::from_ctor(::std::vec![
            $( $crate::lua_table::TableField::bracket(
                $k,
                { const K: bool = $crate::lua_table::__is_lua_const(stringify!($k)); K },
                { const K: bool = $crate::lua_table::__is_lua_const(stringify!($v)); K },
                $v,
            ), )+
        ])
    };
    ($($a:expr),+ ; $($k:ident = $v:expr),* $(,)?) => {
        $crate::lua_table::Table::from_ctor(::std::vec![
            $( $crate::lua_table::TableField::item(
                { const K: bool = $crate::lua_table::__is_lua_const(stringify!($a)); K },
                $a,
            ), )+
            $( $crate::lua_table::TableField::named(
                { static N: ::std::sync::OnceLock<::pob_lua::Name> = ::std::sync::OnceLock::new(); *N.get_or_init(|| ::pob_lua::Name::from(stringify!($k))) },
                { const K: bool = $crate::lua_table::__is_lua_const(stringify!($v)); K },
                $v,
            ), )*
        ])
    };
    ($($a:expr),+ $(,)?) => {
        $crate::lua_table::Table::from_ctor(::std::vec![
            $( $crate::lua_table::TableField::item(
                { const K: bool = $crate::lua_table::__is_lua_const(stringify!($a)); K },
                $a,
            ), )+
        ])
    };
}

/// `actor.output`, `output.MainHand`, minion output: same type and keys.
pub type Output = Table;

#[cfg(test)]
mod tests {
    use super::*;
    use pob_lua::n;

    /// Indexing a non-table names the key it looked up (the crash report's only clue on wasm).
    #[test]
    fn index_error_names_the_key() {
        let obj = Val::Obj(Rc::new(1u8));
        let e = pob_lua::error::catch(|| obj.get(n!("modList"))).unwrap_err();
        assert_eq!(e.0, "attempt to index a userdata value (key 'modList')");
        let e = pob_lua::error::catch(|| Val::Nil.get(3)).unwrap_err();
        assert_eq!(e.0, "attempt to index a nil value (key 3)");
    }

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

    /// HYG1: LuaJIT borders (`luajit` with `jit.off()`; the property test
    /// `tests/hyg1_len.rs` covers 10k random sequences).
    #[test]
    fn luajit_borders() {
        let t = Table::new();
        t.set(1, 1.0);
        t.set(2, 2.0);
        t.set(3, 3.0);
        t.set(2, Val::Nil);
        assert_eq!(t.len(), 1);

        // `{a, b, c}`: BC_TNEW with asize 4.
        let (a, b, c) = (1.0, 2.0, 3.0);
        let t = tbl![a, b, c];
        t.set(2, Val::Nil);
        assert_eq!(t.len(), 3);

        // `{1, 2, 3}`: a template grown by rehashes to asize 5.
        let t = tbl![1.0, 2.0, 3.0];
        t.set(2, Val::Nil);
        assert_eq!(t.len(), 1);

        // `t[1] = 1 t[2] = 2 t[4] = 4` → 4: storing 4 rehashes to asize 5,
        // whose last slot (key 4) is non-nil.
        let t = Table::new();
        for k in [1, 2, 4] {
            t.set(k, 1.0);
        }
        assert_eq!(t.len(), 4);

        // table.insert uses the border: `{1, 2, 3}`, `t[2] = nil`, insert → t[2].
        let t = tbl![1.0, 2.0, 3.0];
        t.set(2, Val::Nil);
        t.push(9.0);
        assert_eq!(t.rawget(2).num(), 9.0);
        assert_eq!(t.len(), 3);

        // copyTable / wipeTable follow LuaJIT's layout rules.
        let t = tbl![a, b, c];
        t.set(2, Val::Nil);
        assert_eq!(t.copyTable(true).len(), 1);
        t.wipe();
        assert_eq!(t.len(), 0);
        t.set(1, 1.0);
        t.set(3, 3.0);
        assert_eq!(t.len(), 3);
    }

    /// `pairs` over a constructor follows field order (§3.4), constant and
    /// run-time fields alike; a duplicate key takes the run-time value
    /// (LuaJIT: `local x = 5; ({a = x, a = 1}).a` and `({a = 1, a = x}).a`
    /// are both 5).
    #[test]
    fn ctor_field_order() {
        let x = 5.0;
        let keys = |t: &Table| t.pairs().map(|(k, _)| k).collect::<Vec<_>>();
        let t = tbl! { a = x, b = 1.0, c = x };
        assert_eq!(keys(&t), [n!("a"), n!("b"), n!("c")].map(Key::S));
        let t = tbl! { [n!("p")] => x, [n!("q")] => 2.0, [10] => x, [11] => 1.0, [12] => x };
        assert_eq!(
            keys(&t),
            [
                Key::S(n!("p")),
                Key::S(n!("q")),
                Key::I(10),
                Key::I(11),
                Key::I(12)
            ]
        );
        let t = tbl! { x, 1.0; a = x, b = 2.0, c = x };
        assert_eq!(
            keys(&t),
            [
                Key::I(1),
                Key::I(2),
                Key::S(n!("a")),
                Key::S(n!("b")),
                Key::S(n!("c"))
            ]
        );
        let t = tbl! { a = x, a = 1.0 };
        assert_eq!(t.rawget(n!("a")).num(), 5.0);
        let t = tbl! { a = 1.0, a = x };
        assert_eq!(t.rawget(n!("a")).num(), 5.0);
        // a run-time nil leaves no key (and no slot) behind
        let nil = Val::Nil;
        let t = tbl! { a = nil.clone(), b = 1.0 };
        assert_eq!(keys(&t), [Key::S(n!("b"))]);
        t.set(n!("a"), 2.0);
        assert_eq!(keys(&t), [Key::S(n!("b")), Key::S(n!("a"))]);
        // items are keyed by position
        let t = tbl![x, Val::Nil, x];
        assert_eq!(keys(&t), [Key::I(1), Key::I(3)]);
    }

    #[test]
    fn lua_const_detection() {
        for s in [
            "1",
            "1.0",
            "-1.5",
            "- 2",
            "1e-5",
            "2.5E+3",
            "0x10",
            "3_000.0",
            "1.0f64",
            "true",
            "false",
            "Val::Nil",
            "Val :: Nil",
            r#""a""#,
            r#""a\"b""#,
            r#"n!("Life")"#,
            r#"n! ("Life")"#,
            " 7 ",
            "0.0",
            "-0.5",
            "-0x10",
            "-0.01e3",
        ] {
            assert!(__is_lua_const(s), "{s:?} is a constant");
        }
        for s in [
            "x",
            "a.b",
            "1.0 + 2.0",
            "f(1)",
            "1.0.max(x)",
            "-x",
            r#""a".to_owned()"#,
            r#""a" == "b""#,
            "n!(x)",
            "Val::from(1.0)",
            "",
            "(1.0)",
            "-0.0",
            "-0",
            "- 0.0",
            "-0e5",
            "-0x0",
            "-0.0f64",
            "-0_0.0_0",
        ] {
            assert!(!__is_lua_const(s), "{s:?} is not a constant");
        }
    }

    /// FUP1: float keys as `lj_tab.c` keeps them (expected values from LuaJIT:
    /// `t[2.0]` is `t[2]`, `t[-0.0]` is `t[0]`, `t[0/0] = v` raises "table index
    /// is NaN" and `t[0/0]` reads nil, a non-integral float is a hash key that
    /// leaves `#t` alone and `pairs` yields it as a number).
    #[test]
    fn float_keys() {
        let t = Table::new();
        t.set(Key::from(929.1), 1.0);
        assert_eq!(t.rawget(Key::from(929.1)).num(), 1.0);
        assert!(t.rawget(Key::from(929.0)).is_nil());
        assert!(t.rawget(929).is_nil());
        assert!(matches!(Key::from(929.1), Key::F(_)));
        // integral floats normalise to the integer key; -0 == 0
        assert_eq!(Key::from(2.0), Key::I(2));
        assert_eq!(Key::from(-0.0), Key::I(0));
        assert_eq!(Key::from(-2.0), Key::I(-2));
        t.set(Key::from(1.0), 10.0);
        t.set(2, 20.0);
        t.set(Key::from(-0.0), 5.0);
        assert_eq!(t.rawget(1).num(), 10.0);
        assert_eq!(t.rawget(Key::from(2.0)).num(), 20.0);
        assert_eq!(t.rawget(0).num(), 5.0);
        assert_eq!(t.len(), 2);
        t.set(Key::from(2.5), 7.0);
        assert_eq!(t.len(), 2);
        // integral values beyond i64 and infinities are float keys
        for x in [1e300, -1e300, f64::INFINITY, f64::NEG_INFINITY, 1.5e-300] {
            let k = Key::from(x);
            assert!(matches!(k, Key::F(_)), "{x}");
            t.set(k, x);
            assert_eq!(t.rawget(Key::from(x)).num(), x);
        }
        // a big integral value inside the i64 range is an integer key
        assert_eq!(
            Key::from(9_007_199_254_740_992.0),
            Key::I(9_007_199_254_740_992)
        );
        // pairs: the array part, then the hash part in insertion order; float
        // keys come back as numbers
        let keys: Vec<Val> = t.pairs().map(|(k, _)| k.to_val()).collect();
        let nums: Vec<f64> = keys.iter().map(Val::num).collect();
        assert_eq!(
            nums,
            [
                1.0,
                2.0,
                929.1,
                0.0,
                2.5,
                1e300,
                -1e300,
                f64::INFINITY,
                f64::NEG_INFINITY,
                1.5e-300
            ]
        );
        // deleting a float key leaves a tombstone, re-adding keeps its slot
        t.set(Key::from(929.1), Val::Nil);
        assert!(t.rawget(Key::from(929.1)).is_nil());
        assert!(t.pairs().all(|(k, _)| k != Key::from(929.1)));
        // NaN
        assert!(Key::try_num(f64::NAN).is_none());
        let r = pob_lua::error::catch(|| Key::from(f64::NAN));
        assert!(r.is_err_and(|e| e.to_string().contains("table index is NaN")));
        // Debug / ordering helpers
        assert_eq!(format!("{:?}", Key::from(929.1)), "F(929.1)");
        assert_eq!(Key::from(0.5).as_num(), Some(0.5));
        assert_eq!(Key::I(3).as_num(), Some(3.0));
        assert_eq!(Key::B(true).as_num(), None);
        // a constructor with a float key (`{[0.5] = 1, 2}`)
        let c = tbl! { [Key::from(0.5)] => 1.0, [1] => 2.0 };
        assert_eq!(c.rawget(Key::from(0.5)).num(), 1.0);
        assert_eq!(c.len(), 1);
        // copyTable keeps float keys
        let d = c.copyTable(false);
        assert_eq!(d.rawget(Key::from(0.5)).num(), 1.0);
    }

    #[test]
    fn errors() {
        let r = pob_lua::error::catch(|| Val::Nil.num());
        assert!(r.is_err());
        let r = pob_lua::error::catch(|| Val::Nil.get(n!("x")));
        assert!(r.is_err());
    }
}
