//! Lua values for the embedded interpreter.
//!
//! Strings carry a precomputed hash so table lookups with string keys (the
//! overwhelming majority in PoB code) never re-hash; table hash parts are
//! insertion-ordered so `pairs` is deterministic and resumable by index.

use std::cell::RefCell;
use std::fmt;
use std::hash::{BuildHasherDefault, Hash, Hasher};
use std::rc::Rc;

use indexmap::IndexMap;

use super::interp::{Interp, LuaError};

pub type NativeFn = dyn Fn(&mut Interp, Vec<Val>) -> Result<Vec<Val>, LuaError>;

// ---- strings ----------------------------------------------------------------

pub struct LStrInner {
    hash: u64,
    s: Box<[u8]>,
}

/// An immutable Lua (byte) string with a cached hash.
#[derive(Clone)]
pub struct LStr(Rc<LStrInner>);

fn hash_bytes(s: &[u8]) -> u64 {
    // FNV-1a — cheap and good enough for table keys.
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in s {
        h ^= u64::from(*b);
        h = h.wrapping_mul(0x0100_0000_01b3);
    }
    h
}

impl LStr {
    #[must_use]
    pub fn new(s: &str) -> Self {
        Self::from_bytes(s.as_bytes())
    }

    #[must_use]
    pub fn from_bytes(b: &[u8]) -> Self {
        Self(Rc::new(LStrInner {
            hash: hash_bytes(b),
            s: b.into(),
        }))
    }

    /// Shared instance for a source-code constant: identical constants
    /// across all loaded chunks compare by pointer.
    #[must_use]
    pub fn intern(s: &str) -> Self {
        Self::intern_bytes(s.as_bytes())
    }

    #[must_use]
    pub fn intern_bytes(b: &[u8]) -> Self {
        thread_local! {
            static INTERNED: RefCell<ahash::AHashMap<Box<[u8]>, LStr>> = RefCell::new(ahash::AHashMap::default());
        }
        INTERNED.with(|m| {
            if let Some(v) = m.borrow().get(b) {
                return v.clone();
            }
            let v = LStr::from_bytes(b);
            m.borrow_mut().insert(b.into(), v.clone());
            v
        })
    }

    #[must_use]
    pub fn hash_value(&self) -> u64 {
        self.0.hash
    }

    #[must_use]
    pub fn as_bytes(&self) -> &[u8] {
        &self.0.s
    }

    /// UTF-8 view; `""` if the bytes aren't valid UTF-8 (binary data).
    #[must_use]
    pub fn as_str(&self) -> &str {
        std::str::from_utf8(&self.0.s).unwrap_or("")
    }

    /// Lossy text view (binary bytes become U+FFFD).
    #[must_use]
    pub fn to_str(&self) -> std::borrow::Cow<'_, str> {
        String::from_utf8_lossy(&self.0.s)
    }

    #[must_use]
    pub fn len(&self) -> usize {
        self.0.s.len()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.0.s.is_empty()
    }
}

impl PartialEq for LStr {
    fn eq(&self, other: &Self) -> bool {
        Rc::ptr_eq(&self.0, &other.0) || (self.0.hash == other.0.hash && self.0.s == other.0.s)
    }
}
impl Eq for LStr {}

impl Hash for LStr {
    fn hash<H: Hasher>(&self, state: &mut H) {
        state.write_u64(self.0.hash);
    }
}

impl From<&str> for LStr {
    fn from(s: &str) -> Self {
        Self::new(s)
    }
}
impl From<String> for LStr {
    fn from(s: String) -> Self {
        Self::new(&s)
    }
}

impl fmt::Display for LStr {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.to_str())
    }
}
impl fmt::Debug for LStr {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:?}", self.to_str())
    }
}

// ---- values -------------------------------------------------------------------

/// A Lua value. Tables and functions have reference semantics.
#[derive(Clone, Default)]
pub enum Val {
    #[default]
    Nil,
    Bool(bool),
    Num(f64),
    Str(LStr),
    Table(TableRef),
    Func(Func),
}

#[derive(Clone)]
pub enum Func {
    Lua(Rc<super::interp::Closure>),
    Native(Rc<NativeFn>),
}

impl Func {
    fn ptr(&self) -> usize {
        match self {
            Func::Lua(c) => Rc::as_ptr(c) as *const u8 as usize,
            Func::Native(f) => Rc::as_ptr(f) as *const u8 as usize,
        }
    }
}

#[derive(Clone)]
pub struct TableRef(pub Rc<RefCell<Table>>);

impl TableRef {
    #[must_use]
    pub fn new() -> Self {
        Self(Rc::new(RefCell::new(Table::default())))
    }

    #[must_use]
    pub fn from_array(items: Vec<Val>) -> Self {
        let t = Self::new();
        t.0.borrow_mut().arr = items;
        t
    }

    #[must_use]
    pub fn get(&self, key: &Val) -> Val {
        self.0.borrow().get(key)
    }

    #[must_use]
    pub fn get_str(&self, key: &str) -> Val {
        self.0.borrow().get_str(key)
    }

    #[must_use]
    pub fn get_int(&self, i: i64) -> Val {
        self.0.borrow().get_int(i)
    }

    pub fn set(&self, key: Val, value: Val) -> Result<(), LuaError> {
        self.0.borrow_mut().set(key, value)
    }

    pub fn set_str(&self, key: &str, value: Val) {
        let _ = self.0.borrow_mut().set(Val::Str(LStr::new(key)), value);
    }

    #[must_use]
    pub fn len(&self) -> usize {
        self.0.borrow().arr.len()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub fn push(&self, v: Val) {
        let mut t = self.0.borrow_mut();
        let n = t.arr.len() as i64 + 1;
        let _ = t.set(Val::Num(n as f64), v);
    }

    /// Snapshot of `pairs(t)`: array part then hash part in insertion order.
    #[must_use]
    pub fn pairs(&self) -> Vec<(Val, Val)> {
        let t = self.0.borrow();
        let mut out: Vec<(Val, Val)> = t
            .arr
            .iter()
            .enumerate()
            .filter(|(_, v)| !v.is_nil())
            .map(|(i, v)| (Val::Num((i + 1) as f64), v.clone()))
            .collect();
        out.extend(
            t.hash
                .iter()
                .map(|(k, (orig, v))| (orig.clone().unwrap_or_else(|| k.to_val()), v.clone())),
        );
        out
    }

    /// `next(t, key)`: the entry after `key` (array part, then hash part).
    pub fn next(&self, key: &Val) -> Result<Option<(Val, Val)>, LuaError> {
        self.0.borrow().next(key)
    }

    /// `ipairs(t)` values.
    #[must_use]
    pub fn array(&self) -> Vec<Val> {
        self.0.borrow().arr.clone()
    }

    #[must_use]
    pub fn ptr_eq(&self, other: &Self) -> bool {
        Rc::ptr_eq(&self.0, &other.0)
    }

    #[must_use]
    pub fn meta(&self) -> Option<TableRef> {
        self.0.borrow().meta.clone()
    }

    /// Raw lookup of a pre-interned string key.
    #[must_use]
    pub fn get_lstr(&self, key: &LStr) -> Val {
        let t = self.0.borrow();
        if t.hash.is_empty() {
            return Val::Nil;
        }
        t.hash
            .get(&Key::Str(key.clone()))
            .map(|(_, v)| v.clone())
            .unwrap_or_default()
    }
}

impl Default for TableRef {
    fn default() -> Self {
        Self::new()
    }
}

/// Hashable table key (non-nil, non-NaN).
#[derive(Clone, PartialEq, Eq)]
pub enum Key {
    Bool(bool),
    /// f64 bits of a non-array number key.
    Num(u64),
    Str(LStr),
    Table(usize),
    Func(usize),
}

impl Hash for Key {
    fn hash<H: Hasher>(&self, state: &mut H) {
        match self {
            Key::Str(s) => state.write_u64(s.hash_value()),
            Key::Bool(b) => state.write_u64(u64::from(*b).wrapping_mul(0x9e37_79b9_7f4a_7c15) ^ 1),
            Key::Num(bits) => state.write_u64(bits.wrapping_mul(0x9e37_79b9_7f4a_7c15) ^ 2),
            Key::Table(p) | Key::Func(p) => {
                state.write_u64((*p as u64).wrapping_mul(0x9e37_79b9_7f4a_7c15) ^ 3)
            }
        }
    }
}

/// Passes the single pre-mixed `u64` a [`Key`] writes straight through.
#[derive(Default)]
pub struct PassHasher(u64);

impl Hasher for PassHasher {
    fn finish(&self) -> u64 {
        self.0
    }
    fn write(&mut self, bytes: &[u8]) {
        for b in bytes {
            self.0 = (self.0 ^ u64::from(*b)).wrapping_mul(0x0100_0000_01b3);
        }
    }
    fn write_u64(&mut self, n: u64) {
        self.0 = n;
    }
}

type KeyMap = IndexMap<Key, (Option<Val>, Val), BuildHasherDefault<PassHasher>>;

impl Key {
    fn to_val(&self) -> Val {
        match self {
            Key::Bool(b) => Val::Bool(*b),
            Key::Num(bits) => Val::Num(f64::from_bits(*bits)),
            Key::Str(s) => Val::Str(s.clone()),
            Key::Table(_) | Key::Func(_) => Val::Nil,
        }
    }
}

/// Lua table: a 1-based array part plus an insertion-ordered hash part.
/// Hash entries for table/function keys also keep the original key value.
#[derive(Default)]
pub struct Table {
    pub arr: Vec<Val>,
    /// Deleted keys stay as `nil` tombstones so `next` keeps its place during
    /// traversal (Lua allows clearing fields while iterating); compacted on
    /// the next insertion of a new key.
    pub hash: KeyMap,
    tombstones: usize,
    /// `setmetatable` target; only `__index`, `__newindex` and `__call`
    /// are honoured.
    pub meta: Option<TableRef>,
}

fn num_key(n: f64) -> Key {
    // Normalise -0.0 so it equals 0.0.
    let n = if n == 0.0 { 0.0 } else { n };
    Key::Num(n.to_bits())
}

impl Table {
    fn array_index(key: &Val) -> Option<usize> {
        if let Val::Num(n) = key {
            if n.fract() == 0.0 && *n >= 1.0 && *n <= 1e15 {
                return Some(*n as usize);
            }
        }
        None
    }

    #[must_use]
    pub fn get(&self, key: &Val) -> Val {
        match key {
            Val::Str(s) => {
                if self.hash.is_empty() {
                    return Val::Nil;
                }
                self.hash
                    .get(&Key::Str(s.clone()))
                    .map(|(_, v)| v.clone())
                    .unwrap_or_default()
            }
            Val::Num(_) => {
                if let Some(i) = Self::array_index(key) {
                    if i <= self.arr.len() {
                        return self.arr[i - 1].clone();
                    }
                }
                match to_key(key) {
                    Some(k) => self
                        .hash
                        .get(&k)
                        .map(|(_, v)| v.clone())
                        .unwrap_or_default(),
                    None => Val::Nil,
                }
            }
            _ => match to_key(key) {
                Some(k) => self
                    .hash
                    .get(&k)
                    .map(|(_, v)| v.clone())
                    .unwrap_or_default(),
                None => Val::Nil,
            },
        }
    }

    #[must_use]
    pub fn get_str(&self, key: &str) -> Val {
        self.hash
            .get(&Key::Str(LStr::new(key)))
            .map(|(_, v)| v.clone())
            .unwrap_or_default()
    }

    #[must_use]
    pub fn get_int(&self, i: i64) -> Val {
        if i >= 1 && (i as usize) <= self.arr.len() {
            return self.arr[i as usize - 1].clone();
        }
        self.hash
            .get(&num_key(i as f64))
            .map(|(_, v)| v.clone())
            .unwrap_or_default()
    }

    pub fn set(&mut self, key: Val, value: Val) -> Result<(), LuaError> {
        if let Some(i) = Self::array_index(&key) {
            let len = self.arr.len();
            if i <= len {
                self.arr[i - 1] = value;
                // Trailing nils shrink the array (keeps `#t` a border).
                while self.arr.last().is_some_and(Val::is_nil) {
                    self.arr.pop();
                }
                return Ok(());
            }
            if i == len + 1 {
                if value.is_nil() {
                    return Ok(());
                }
                self.arr.push(value);
                // Migrate following integer keys from the hash part.
                if !self.hash.is_empty() {
                    loop {
                        let next = num_key((self.arr.len() + 1) as f64);
                        match self.hash.shift_remove(&next) {
                            Some((_, v)) if !v.is_nil() => self.arr.push(v),
                            Some(_) => {
                                self.tombstones -= 1;
                                break;
                            }
                            None => break,
                        }
                    }
                }
                return Ok(());
            }
        }
        let k = to_key(&key).ok_or_else(|| LuaError::msg("table index is nil or NaN"))?;
        if value.is_nil() {
            if let Some(slot) = self.hash.get_mut(&k) {
                if !slot.1.is_nil() {
                    slot.1 = Val::Nil;
                    self.tombstones += 1;
                }
            }
        } else {
            match self.hash.get_mut(&k) {
                Some(slot) => {
                    if slot.1.is_nil() {
                        self.tombstones -= 1;
                    }
                    slot.1 = value;
                }
                None => {
                    if self.tombstones > 8 && self.tombstones * 2 > self.hash.len() {
                        self.hash.retain(|_, (_, v)| !v.is_nil());
                        self.tombstones = 0;
                    }
                    let orig = matches!(k, Key::Table(_) | Key::Func(_)).then_some(key);
                    self.hash.insert(k, (orig, value));
                }
            }
        }
        Ok(())
    }

    /// `next(t, key)`.
    pub fn next(&self, key: &Val) -> Result<Option<(Val, Val)>, LuaError> {
        // Position after `key`: array indices first, then hash indices.
        let mut arr_from = 0usize;
        let mut hash_from = 0usize;
        match key {
            Val::Nil => {}
            _ => {
                if let Some(i) = Self::array_index(key) {
                    if i <= self.arr.len() {
                        arr_from = i;
                    } else {
                        arr_from = self.arr.len();
                        // Trimmed trailing array slot, or a hash-part integer key.
                        hash_from = self.hash_index_after(key).unwrap_or(0);
                    }
                } else {
                    arr_from = self.arr.len();
                    hash_from = self.hash_index_after(key)?;
                }
            }
        }
        for i in arr_from..self.arr.len() {
            if !self.arr[i].is_nil() {
                return Ok(Some((Val::Num((i + 1) as f64), self.arr[i].clone())));
            }
        }
        let mut idx = hash_from;
        while let Some((k, (orig, v))) = self.hash.get_index(idx) {
            if !v.is_nil() {
                return Ok(Some((
                    orig.clone().unwrap_or_else(|| k.to_val()),
                    v.clone(),
                )));
            }
            idx += 1;
        }
        Ok(None)
    }

    fn hash_index_after(&self, key: &Val) -> Result<usize, LuaError> {
        let k = to_key(key).ok_or_else(|| LuaError::msg("invalid key to 'next'"))?;
        match self.hash.get_index_of(&k) {
            Some(i) => Ok(i + 1),
            // Key removed during traversal: PoB never relies on this.
            None => Err(LuaError::msg("invalid key to 'next'")),
        }
    }
}

fn to_key(v: &Val) -> Option<Key> {
    Some(match v {
        Val::Nil => return None,
        Val::Bool(b) => Key::Bool(*b),
        Val::Num(n) => {
            if n.is_nan() {
                return None;
            }
            num_key(*n)
        }
        Val::Str(s) => Key::Str(s.clone()),
        Val::Table(t) => Key::Table(Rc::as_ptr(&t.0) as *const u8 as usize),
        Val::Func(f) => Key::Func(f.ptr()),
    })
}

impl Val {
    #[must_use]
    pub fn str(s: &str) -> Self {
        Val::Str(LStr::new(s))
    }

    #[must_use]
    pub fn bytes(b: &[u8]) -> Self {
        Val::Str(LStr::from_bytes(b))
    }

    #[must_use]
    pub fn is_nil(&self) -> bool {
        matches!(self, Val::Nil)
    }

    #[must_use]
    pub fn truthy(&self) -> bool {
        !matches!(self, Val::Nil | Val::Bool(false))
    }

    #[must_use]
    pub fn type_name(&self) -> &'static str {
        match self {
            Val::Nil => "nil",
            Val::Bool(_) => "boolean",
            Val::Num(_) => "number",
            Val::Str(_) => "string",
            Val::Table(_) => "table",
            Val::Func(_) => "function",
        }
    }

    #[must_use]
    pub fn as_num(&self) -> Option<f64> {
        match self {
            Val::Num(n) => Some(*n),
            _ => None,
        }
    }

    /// Arithmetic coercion: numbers, and strings that parse as numbers.
    #[must_use]
    pub fn to_num(&self) -> Option<f64> {
        match self {
            Val::Num(n) => Some(*n),
            Val::Str(s) => crate::lua::lua_pattern::to_number(s.as_str()),
            _ => None,
        }
    }

    #[must_use]
    pub fn as_str(&self) -> Option<&str> {
        match self {
            Val::Str(s) => Some(s.as_str()),
            _ => None,
        }
    }

    #[must_use]
    pub fn as_table(&self) -> Option<&TableRef> {
        match self {
            Val::Table(t) => Some(t),
            _ => None,
        }
    }

    /// Raw equality (`==` without metamethods).
    #[must_use]
    pub fn raw_eq(&self, other: &Val) -> bool {
        match (self, other) {
            (Val::Nil, Val::Nil) => true,
            (Val::Bool(a), Val::Bool(b)) => a == b,
            (Val::Num(a), Val::Num(b)) => a == b,
            (Val::Str(a), Val::Str(b)) => a == b,
            (Val::Table(a), Val::Table(b)) => a.ptr_eq(b),
            (Val::Func(a), Val::Func(b)) => a.ptr() == b.ptr(),
            _ => false,
        }
    }

    #[must_use]
    pub fn native(
        f: impl Fn(&mut Interp, Vec<Val>) -> Result<Vec<Val>, LuaError> + 'static,
    ) -> Self {
        Val::Func(Func::Native(Rc::new(f)))
    }
}

impl PartialEq for Val {
    fn eq(&self, other: &Self) -> bool {
        self.raw_eq(other)
    }
}

/// Lua's `tostring` for numbers (`%.14g`).
#[must_use]
pub fn num_to_string(n: f64) -> String {
    if n.fract() == 0.0 && n.abs() < 1e15 {
        return format!("{}", n as i64);
    }
    if n.is_infinite() {
        return if n > 0.0 { "inf".into() } else { "-inf".into() };
    }
    if n.is_nan() {
        return "nan".into();
    }
    // %.14g
    let s = format!("{:.13e}", n);
    let (mant, exp) = s.split_once('e').unwrap_or((&s, "0"));
    let exp: i32 = exp.parse().unwrap_or(0);
    if (-5..14).contains(&exp) {
        let decimals = (13 - exp).max(0) as usize;
        let mut out = format!("{:.*}", decimals, n);
        if out.contains('.') {
            while out.ends_with('0') {
                out.pop();
            }
            if out.ends_with('.') {
                out.pop();
            }
        }
        out
    } else {
        let mut m = mant.to_owned();
        if m.contains('.') {
            while m.ends_with('0') {
                m.pop();
            }
            if m.ends_with('.') {
                m.pop();
            }
        }
        format!("{m}e{}{:02}", if exp < 0 { '-' } else { '+' }, exp.abs())
    }
}

impl fmt::Display for Val {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Val::Nil => f.write_str("nil"),
            Val::Bool(b) => write!(f, "{b}"),
            Val::Num(n) => f.write_str(&num_to_string(*n)),
            Val::Str(s) => f.write_str(&s.to_str()),
            Val::Table(t) => write!(f, "table: {:p}", Rc::as_ptr(&t.0)),
            Val::Func(func) => write!(f, "function: {:#x}", func.ptr()),
        }
    }
}

impl fmt::Debug for Val {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Val::Str(s) => write!(f, "{s:?}"),
            Val::Table(t) => {
                let t = t.0.borrow();
                write!(f, "{{")?;
                for v in &t.arr {
                    write!(f, "{v:?}, ")?;
                }
                for (k, (_, v)) in &t.hash {
                    write!(f, "{:?}={v:?}, ", k.to_val())?;
                }
                write!(f, "}}")
            }
            other => write!(f, "{other}"),
        }
    }
}
