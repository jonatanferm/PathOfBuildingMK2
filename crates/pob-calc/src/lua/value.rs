//! Lua values for the embedded interpreter.

use std::cell::RefCell;
use std::fmt;
use std::hash::{Hash, Hasher};
use std::rc::Rc;

use indexmap::IndexMap;

use super::interp::{Interp, LuaError};

pub type NativeFn = dyn Fn(&mut Interp, Vec<Val>) -> Result<Vec<Val>, LuaError>;

/// A Lua value. Tables and functions have reference semantics.
#[derive(Clone, Default)]
pub enum Val {
    #[default]
    Nil,
    Bool(bool),
    Num(f64),
    Str(Rc<str>),
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
        let _ = self.0.borrow_mut().set(Val::Str(key.into()), value);
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
        out.extend(t.hash.iter().map(|(k, v)| (k.to_val(), v.clone())));
        out
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
}

impl Default for TableRef {
    fn default() -> Self {
        Self::new()
    }
}

/// Hashable table key (non-nil, non-NaN).
#[derive(Clone, PartialEq, Eq, Hash)]
pub enum Key {
    Bool(bool),
    /// f64 bits of a non-array number key.
    Num(u64),
    Str(Rc<str>),
    Table(usize),
    Func(usize),
}

impl Key {
    fn to_val(&self) -> Val {
        match self {
            Key::Bool(b) => Val::Bool(*b),
            Key::Num(bits) => Val::Num(f64::from_bits(*bits)),
            Key::Str(s) => Val::Str(s.clone()),
            // Reference keys can't be turned back into values here; PoB's
            // parser never iterates tables keyed by tables/functions.
            Key::Table(_) | Key::Func(_) => Val::Nil,
        }
    }
}

/// Lua table: a 1-based array part plus a hash part.
#[derive(Default)]
pub struct Table {
    pub arr: Vec<Val>,
    pub hash: IndexMap<Key, Val, ahash::RandomState>,
    /// Keys for reference-typed keys (kept alive so `to_val` isn't needed).
    ref_keys: Vec<(Key, Val)>,
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
        if let Some(i) = Self::array_index(key) {
            if i <= self.arr.len() {
                return self.arr[i - 1].clone();
            }
        }
        match to_key(key) {
            Some(k) => self.hash.get(&k).cloned().unwrap_or_default(),
            None => Val::Nil,
        }
    }

    #[must_use]
    pub fn get_str(&self, key: &str) -> Val {
        self.hash
            .get(&Key::Str(key.into()))
            .cloned()
            .unwrap_or_default()
    }

    #[must_use]
    pub fn get_int(&self, i: i64) -> Val {
        if i >= 1 && (i as usize) <= self.arr.len() {
            return self.arr[i as usize - 1].clone();
        }
        self.hash
            .get(&num_key(i as f64))
            .cloned()
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
                loop {
                    let next = num_key((self.arr.len() + 1) as f64);
                    match self.hash.shift_remove(&next) {
                        Some(v) => self.arr.push(v),
                        None => break,
                    }
                }
                return Ok(());
            }
        }
        let k = to_key(&key).ok_or_else(|| LuaError::msg("table index is nil or NaN"))?;
        if value.is_nil() {
            self.hash.shift_remove(&k);
        } else {
            if matches!(k, Key::Table(_) | Key::Func(_)) {
                self.ref_keys.push((k.clone(), key));
            }
            self.hash.insert(k, value);
        }
        Ok(())
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
        Val::Str(s.into())
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
            Val::Str(s) => crate::lua_pattern::to_number(s),
            _ => None,
        }
    }

    #[must_use]
    pub fn as_str(&self) -> Option<&str> {
        match self {
            Val::Str(s) => Some(s),
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

impl Hash for Val {
    fn hash<H: Hasher>(&self, state: &mut H) {
        if let Some(k) = to_key(self) {
            k.hash(state);
        }
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
            Val::Str(s) => f.write_str(s),
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
                for (k, v) in &t.hash {
                    write!(f, "{:?}={v:?}, ", k.to_val())?;
                }
                write!(f, "}}")
            }
            other => write!(f, "{other}"),
        }
    }
}
