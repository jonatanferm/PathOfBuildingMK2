//! Immutable Lua values carried by mods and data (calc doc §4.3), and the
//! generic dump value [`DataValue`] for data roots that are not typed.
//!
//! [`Value`] is what a mod's `value` (and a tag-free Lua table inside it)
//! holds. It is `Send + Sync`: mods live in the process-global `Data` and in
//! `Arc`s shared across stores. Equality is structural (Lua compares tables by
//! identity; no PoB code compares mod values that are tables with `==`).
//!
//! [`DataValue`] mirrors a dumped Lua value exactly (crates/pob-dump/FORMAT.md):
//! string keys, the `1..n` sequence part, other numeric keys, other keys and
//! the metatable. References to other tables stay [`DataValue::Ref`] (the
//! canonical path), which keeps cycles (`statMap._grantedEffect`) harmless; the
//! loader resolves them through its path index.

use std::sync::Arc;

use crate::modifier::{JewelFuncSpec, ModRef};
use crate::{LuaFnRef, Name};

/// Shared immutable string (non-key text: descriptions, sources, item text).
pub type AStr = Arc<str>;

/// Immutable Lua value carried by mods and data (Send + Sync).
#[derive(Debug, Clone, PartialEq, Default)]
pub enum Value {
    #[default]
    Nil,
    Bool(bool),
    Num(f64),
    Str(AStr),
    /// The `mod` member of `{ mod = … }` payloads (and a bare mod used as a value).
    Mod(ModRef),
    /// `{ key=…, value=… }`, `{ mod=…, onlyAllies=… }`, arrays …
    Table(Arc<ValueTable>),
    /// Replaces the Lua closure table `{ type = …, func = function … }` of
    /// `JewelFunc` / `ExtraJewelFunc` LIST mods.
    JewelFunc(Arc<JewelFuncSpec>),
}

/// A Lua table inside a [`Value`]: the `1..n` part plus string keys in dump
/// order. Other numeric keys (rare) are kept as fields named `"#<n>"`
/// (calc doc §9.1 convention) by the loader.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct ValueTable {
    pub array: Vec<Value>,
    pub fields: Vec<(Name, Value)>,
}

static NIL: Value = Value::Nil;

impl ValueTable {
    pub fn new() -> Self {
        Self::default()
    }

    /// `t[k]` for a string key (`nil` when absent).
    pub fn get(&self, k: Name) -> &Value {
        self.fields
            .iter()
            .find(|(n, _)| *n == k)
            .map_or(&NIL, |(_, v)| v)
    }

    /// `t[i]` for an array index (1-based).
    pub fn index(&self, i1: usize) -> &Value {
        if i1 == 0 {
            return &NIL;
        }
        self.array.get(i1 - 1).unwrap_or(&NIL)
    }

    /// A copy with `t[k] = v` (a Nil `v` removes the key).
    pub fn with(&self, k: Name, v: Value) -> ValueTable {
        let mut t = self.clone();
        t.set(k, v);
        t
    }

    /// In-place `t[k] = v` for tables under construction (a Nil `v` removes the key).
    pub fn set(&mut self, k: Name, v: Value) {
        if let Some(pos) = self.fields.iter().position(|(n, _)| *n == k) {
            if matches!(v, Value::Nil) {
                self.fields.remove(pos);
            } else {
                self.fields[pos].1 = v;
            }
        } else if !matches!(v, Value::Nil) {
            self.fields.push((k, v));
        }
    }

    /// `#t` (the array part is hole-free).
    pub fn len(&self) -> usize {
        self.array.len()
    }

    pub fn is_empty(&self) -> bool {
        self.array.is_empty() && self.fields.is_empty()
    }
}

impl Value {
    /// Lua truthiness: everything except `nil` and `false`.
    #[inline]
    pub fn truthy(&self) -> bool {
        !matches!(self, Value::Nil | Value::Bool(false))
    }

    /// `Some(n)` for a number only (no string coercion).
    #[inline]
    pub fn num(&self) -> Option<f64> {
        match self {
            Value::Num(n) => Some(*n),
            _ => None,
        }
    }

    pub fn is_nil(&self) -> bool {
        matches!(self, Value::Nil)
    }

    pub fn as_str(&self) -> Option<&str> {
        match self {
            Value::Str(s) => Some(s),
            _ => None,
        }
    }

    pub fn as_table(&self) -> Option<&Arc<ValueTable>> {
        match self {
            Value::Table(t) => Some(t),
            _ => None,
        }
    }

    pub fn as_mod(&self) -> Option<&ModRef> {
        match self {
            Value::Mod(m) => Some(m),
            _ => None,
        }
    }

    /// `v[k]` on a table value; `nil` for every other kind (callers that index a
    /// non-table where Lua would raise must check the kind themselves).
    pub fn get(&self, k: Name) -> &Value {
        match self {
            Value::Table(t) => t.get(k),
            _ => &NIL,
        }
    }

    /// `value.mod` of a `{ mod = … }` table value.
    pub fn inner_mod(&self) -> Option<&ModRef> {
        match self {
            Value::Table(t) => t.get(pob_lua::n!("mod")).as_mod(),
            _ => None,
        }
    }

    /// Lua `type(v)`.
    pub fn lua_type(&self) -> &'static str {
        match self {
            Value::Nil => "nil",
            Value::Bool(_) => "boolean",
            Value::Num(_) => "number",
            Value::Str(_) => "string",
            Value::Mod(_) | Value::Table(_) | Value::JewelFunc(_) => "table",
        }
    }
}

impl From<f64> for Value {
    fn from(n: f64) -> Self {
        Value::Num(n)
    }
}
impl From<i32> for Value {
    fn from(n: i32) -> Self {
        Value::Num(f64::from(n))
    }
}
impl From<bool> for Value {
    fn from(b: bool) -> Self {
        Value::Bool(b)
    }
}
impl From<&str> for Value {
    fn from(s: &str) -> Self {
        Value::Str(Arc::from(s))
    }
}
impl From<String> for Value {
    fn from(s: String) -> Self {
        Value::Str(Arc::from(s))
    }
}
impl From<AStr> for Value {
    fn from(s: AStr) -> Self {
        Value::Str(s)
    }
}
impl From<Name> for Value {
    fn from(s: Name) -> Self {
        Value::Str(Arc::from(s.as_str()))
    }
}
impl From<Option<Name>> for Value {
    fn from(s: Option<Name>) -> Self {
        s.map_or(Value::Nil, Value::from)
    }
}
impl From<ModRef> for Value {
    fn from(m: ModRef) -> Self {
        Value::Mod(m)
    }
}
impl From<crate::Mod> for Value {
    fn from(m: crate::Mod) -> Self {
        Value::Mod(Arc::new(m))
    }
}
impl From<ValueTable> for Value {
    fn from(t: ValueTable) -> Self {
        Value::Table(Arc::new(t))
    }
}
impl From<Option<f64>> for Value {
    fn from(n: Option<f64>) -> Self {
        n.map_or(Value::Nil, Value::Num)
    }
}

// ---------------------------------------------------------------------------
// DataValue: generic dumped Lua values
// ---------------------------------------------------------------------------

/// A dumped Lua value, exactly as crates/pob-dump/FORMAT.md encodes it.
/// Used for data roots that are not typed (contracts.md lists them) and as the
/// loader's intermediate tree.
#[derive(Debug, Clone, PartialEq, Default, serde::Serialize, serde::Deserialize)]
pub enum DataValue {
    #[default]
    Nil,
    Bool(bool),
    Num(f64),
    Str(AStr),
    Table(Arc<DataTable>),
    Fn(Arc<LuaFnRef>),
    /// `{"$ref": "<canonical path>"}`: resolved by the loader's index.
    Ref(AStr),
}

/// A dumped Lua table (members in dump order: string keys bytewise, then the
/// sequence, other numeric keys ascending, other keys, metatable).
#[derive(Debug, Clone, PartialEq, Default, serde::Serialize, serde::Deserialize)]
pub struct DataTable {
    /// String keys (escaped `$$` keys unescaped).
    pub fields: Vec<(AStr, DataValue)>,
    /// Keys `1..n` (`$seq`, or the JSON array).
    pub seq: Vec<DataValue>,
    /// Every other numeric key (`$num`).
    pub num: Vec<(f64, DataValue)>,
    /// Boolean / table / function keys (`$keys`).
    pub keys: Vec<(DataValue, DataValue)>,
    /// The metatable (`$meta`).
    pub meta: Option<DataValue>,
}

static DNIL: DataValue = DataValue::Nil;

impl DataTable {
    pub fn field(&self, k: &str) -> &DataValue {
        self.fields
            .iter()
            .find(|(n, _)| &**n == k)
            .map_or(&DNIL, |(_, v)| v)
    }

    /// `t[i]` for an integer key (sequence part first, then `$num`).
    pub fn index(&self, i: f64) -> &DataValue {
        if i >= 1.0 && i.fract() == 0.0 && (i as usize) <= self.seq.len() {
            return &self.seq[i as usize - 1];
        }
        self.num
            .iter()
            .find(|(n, _)| *n == i)
            .map_or(&DNIL, |(_, v)| v)
    }

    pub fn is_empty(&self) -> bool {
        self.fields.is_empty() && self.seq.is_empty() && self.num.is_empty() && self.keys.is_empty()
    }
}

impl DataValue {
    pub fn truthy(&self) -> bool {
        !matches!(self, DataValue::Nil | DataValue::Bool(false))
    }
    pub fn num(&self) -> Option<f64> {
        match self {
            DataValue::Num(n) => Some(*n),
            _ => None,
        }
    }
    pub fn as_str(&self) -> Option<&str> {
        match self {
            DataValue::Str(s) => Some(s),
            _ => None,
        }
    }
    pub fn table(&self) -> Option<&Arc<DataTable>> {
        match self {
            DataValue::Table(t) => Some(t),
            _ => None,
        }
    }
    /// `v.k` on a table (no reference resolution; see `loader::DataIndex`).
    pub fn field(&self, k: &str) -> &DataValue {
        match self {
            DataValue::Table(t) => t.field(k),
            _ => &DNIL,
        }
    }
}
