//! Loading the dump into [`Data`](crate::Data) (calc doc §4.4, §9.1 step 4).
//!
//! Pipeline (compile time, `pob-dump compile`, crates/pob-dump/src/compile.rs):
//!
//! 1. every dump file is parsed into a [`DataValue`] tree ([`parse_json`]):
//!    `$ref` stays a [`DataValue::Ref`] to its canonical path, `$fn` becomes a
//!    [`LuaFnRef`], `$float` a number;
//! 2. the trees are kept as a [`DumpTree`] (roots + the canonical paths every
//!    `$ref` targets), which [`encode_bin`] writes as `data.bin` (postcard);
//! 3. [`Data`](crate::Data) is deserialised from the tree through
//!    [`TreeDeserializer`], a serde deserializer that presents each table in
//!    the dump's JSON shape (string keys, then the pseudo-members `$seq`,
//!    `$num`, `$keys`, `$meta`), resolves references transparently, shares
//!    `Arc`s by canonical path ([`shared`]) and reports every error with its
//!    Lua path. Every schema struct is `#[serde(deny_unknown_fields)]`, so a
//!    key the schema does not model fails the load ("zero unknown keys").
//!
//! At runtime [`crate::init`] decodes `data.bin` and runs step 3 again.

use std::any::{Any, TypeId};
use std::cell::RefCell;
use std::collections::HashMap;
use std::fmt;
use std::fmt::Write as _;
use std::sync::Arc;

use serde::de::{
    self, DeserializeSeed, Deserializer, IntoDeserializer, MapAccess, SeqAccess, Visitor,
};
use serde::Deserialize;

use crate::value::{AStr, DataTable, DataValue};
use crate::LuaFnRef;

// ---------------------------------------------------------------------------
// Errors
// ---------------------------------------------------------------------------

/// A load error, with the Lua path of the offending value when known.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LoadError {
    pub msg: String,
    pub path: Option<String>,
}

impl LoadError {
    pub fn new(msg: impl Into<String>) -> Self {
        LoadError {
            msg: msg.into(),
            path: None,
        }
    }
}

impl fmt::Display for LoadError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.path {
            Some(p) => write!(f, "{p}: {}", self.msg),
            None => f.write_str(&self.msg),
        }
    }
}

impl std::error::Error for LoadError {}

impl de::Error for LoadError {
    fn custom<T: fmt::Display>(msg: T) -> Self {
        LoadError::new(msg.to_string())
    }
}

fn at(e: LoadError, path: &Path<'_>) -> LoadError {
    if e.path.is_some() {
        e
    } else {
        LoadError {
            msg: e.msg,
            path: Some(path.render()),
        }
    }
}

// ---------------------------------------------------------------------------
// Canonical number text and path syntax (crates/pob-dump/FORMAT.md)
// ---------------------------------------------------------------------------

/// The dump's canonical text of a number (`numKey` in walker.lua).
pub fn num_text(v: f64) -> String {
    if v.is_nan() {
        return "nan".into();
    }
    if v == f64::INFINITY {
        return "inf".into();
    }
    if v == f64::NEG_INFINITY {
        return "-inf".into();
    }
    if v == v.floor() && v > -9_007_199_254_740_992.0 && v < 9_007_199_254_740_992.0 {
        if v == 0.0 && v.is_sign_negative() {
            return "-0".into();
        }
        return format!("{}", v as i64);
    }
    for f in ["%.15g", "%.16g", "%.17g"] {
        let s = pob_lua::lfmt!(f, v);
        if pob_lua::tonumber(&s) == Some(v) {
            return s;
        }
    }
    pob_lua::lfmt!("%.17g", v)
}

const LUA_KEYWORDS: &[&str] = &[
    "and", "break", "do", "else", "elseif", "end", "false", "for", "function", "goto", "if", "in",
    "local", "nil", "not", "or", "repeat", "return", "then", "true", "until", "while",
];

fn is_ident(k: &str) -> bool {
    let b = k.as_bytes();
    !b.is_empty()
        && (b[0].is_ascii_alphabetic() || b[0] == b'_')
        && b.iter().all(|c| c.is_ascii_alphanumeric() || *c == b'_')
        && !LUA_KEYWORDS.contains(&k)
}

/// `jstr` of walker.lua: JSON string with `%c`, `"` and `\` escaped.
fn jstr(k: &str, out: &mut String) {
    out.push('"');
    for c in k.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\u{8}' => out.push_str("\\b"),
            '\u{c}' => out.push_str("\\f"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 || c as u32 == 0x7f => {
                let _ = write!(out, "\\u{:04x}", c as u32);
            }
            c => out.push(c),
        }
    }
    out.push('"');
}

/// Path component for a string key (`strKeyPath` of walker.lua).
pub fn str_key_path(k: &str, out: &mut String) {
    if is_ident(k) {
        out.push('.');
        out.push_str(k);
    } else {
        out.push('[');
        jstr(k, out);
        out.push(']');
    }
}

/// A Lua path being walked, as a parent-linked list on the stack (rendered
/// only for errors and shared-`Arc` keys).
#[derive(Clone, Copy)]
pub enum Path<'p> {
    Root(&'p str),
    Key(&'p Path<'p>, &'p str),
    Index(&'p Path<'p>, usize),
    Num(&'p Path<'p>, f64),
    Meta(&'p Path<'p>),
    Pseudo(&'p Path<'p>, &'static str),
}

impl Path<'_> {
    pub fn render(&self) -> String {
        let mut s = String::new();
        self.render_into(&mut s);
        s
    }

    fn render_into(&self, s: &mut String) {
        match self {
            Path::Root(r) => s.push_str(r),
            Path::Key(p, k) => {
                p.render_into(s);
                str_key_path(k, s);
            }
            Path::Index(p, i) => {
                p.render_into(s);
                let _ = write!(s, "[{i}]");
            }
            Path::Num(p, n) => {
                p.render_into(s);
                s.push('[');
                s.push_str(&num_text(*n));
                s.push(']');
            }
            Path::Meta(p) => {
                p.render_into(s);
                s.push_str("<meta>");
            }
            // pseudo members ($seq/$num) do not appear in canonical paths
            Path::Pseudo(p, _) => p.render_into(s),
        }
    }
}

// ---------------------------------------------------------------------------
// JSON → DataValue
// ---------------------------------------------------------------------------

struct JsonValue(DataValue);

impl<'de> Deserialize<'de> for JsonValue {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        d.deserialize_any(JsonVisitor).map(JsonValue)
    }
}

struct JsonVisitor;

fn hex_decode(s: &str) -> Option<Vec<u8>> {
    (0..s.len())
        .step_by(2)
        .map(|i| s.get(i..i + 2).and_then(|h| u8::from_str_radix(h, 16).ok()))
        .collect()
}

impl<'de> Visitor<'de> for JsonVisitor {
    type Value = DataValue;
    fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.write_str("a dumped Lua value")
    }
    fn visit_bool<E>(self, b: bool) -> Result<DataValue, E> {
        Ok(DataValue::Bool(b))
    }
    fn visit_i64<E>(self, n: i64) -> Result<DataValue, E> {
        Ok(DataValue::Num(n as f64))
    }
    fn visit_u64<E>(self, n: u64) -> Result<DataValue, E> {
        Ok(DataValue::Num(n as f64))
    }
    fn visit_f64<E>(self, n: f64) -> Result<DataValue, E> {
        Ok(DataValue::Num(n))
    }
    fn visit_str<E>(self, s: &str) -> Result<DataValue, E> {
        Ok(DataValue::Str(AStr::from(s)))
    }
    fn visit_unit<E>(self) -> Result<DataValue, E> {
        Ok(DataValue::Nil)
    }
    fn visit_seq<A: SeqAccess<'de>>(self, mut a: A) -> Result<DataValue, A::Error> {
        let mut seq = Vec::new();
        while let Some(JsonValue(v)) = a.next_element()? {
            seq.push(v);
        }
        Ok(DataValue::Table(Arc::new(DataTable {
            seq,
            ..Default::default()
        })))
    }
    fn visit_map<A: MapAccess<'de>>(self, mut a: A) -> Result<DataValue, A::Error> {
        let mut t = DataTable::default();
        let mut fnref: Option<(String, Option<String>, Option<String>)> = None;
        while let Some(k) = a.next_key::<String>()? {
            match k.as_str() {
                "$ref" => {
                    let p: String = a.next_value()?;
                    return finish_single(a, DataValue::Ref(AStr::from(p)));
                }
                "$float" => {
                    let s: String = a.next_value()?;
                    let n = crate::modifier::parse_float_marker(&s)
                        .ok_or_else(|| de::Error::custom("bad $float"))?;
                    return finish_single(a, DataValue::Num(n));
                }
                "$bytes" => {
                    let s: String = a.next_value()?;
                    let b = hex_decode(&s).ok_or_else(|| de::Error::custom("bad $bytes"))?;
                    return finish_single(
                        a,
                        DataValue::Str(AStr::from(String::from_utf8_lossy(&b).as_ref())),
                    );
                }
                "$fn" => fnref = Some((a.next_value()?, None, None)),
                "src" if fnref.is_some() => fnref.as_mut().unwrap().1 = Some(a.next_value()?),
                "hash" if fnref.is_some() => fnref.as_mut().unwrap().2 = Some(a.next_value()?),
                "$seq" => {
                    let JsonValue(v) = a.next_value()?;
                    match v {
                        DataValue::Table(s) if s.fields.is_empty() => t.seq = s.seq.clone(),
                        _ => return Err(de::Error::custom("$seq is not an array")),
                    }
                }
                "$num" => {
                    let m: NumMap = a.next_value()?;
                    t.num = m.0;
                }
                "$keys" => {
                    let l: Vec<(JsonValue, JsonValue)> = a.next_value()?;
                    t.keys = l.into_iter().map(|(k, v)| (k.0, v.0)).collect();
                }
                "$meta" => {
                    let JsonValue(v) = a.next_value()?;
                    t.meta = Some(v);
                }
                _ => {
                    let key = if let Some(rest) = k.strip_prefix("$$") {
                        format!("${rest}")
                    } else if k.starts_with('$') {
                        return Err(de::Error::custom(format!("unknown reserved member {k}")));
                    } else {
                        k
                    };
                    let JsonValue(v) = a.next_value()?;
                    t.fields.push((AStr::from(key), v));
                }
            }
        }
        if let Some((id, src, hash)) = fnref {
            if !t.is_empty() || t.meta.is_some() {
                return Err(de::Error::custom("$fn object with other members"));
            }
            return Ok(DataValue::Fn(Arc::new(LuaFnRef {
                id: crate::Name::from(id.as_str()),
                src: src.unwrap_or_default(),
                hash: hash.unwrap_or_default(),
            })));
        }
        Ok(DataValue::Table(Arc::new(t)))
    }
}

fn finish_single<'de, A: MapAccess<'de>>(mut a: A, v: DataValue) -> Result<DataValue, A::Error> {
    if a.next_key::<String>()?.is_some() {
        return Err(de::Error::custom("special object with extra members"));
    }
    Ok(v)
}

struct NumMap(Vec<(f64, DataValue)>);
impl<'de> Deserialize<'de> for NumMap {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        struct V;
        impl<'de> Visitor<'de> for V {
            type Value = NumMap;
            fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
                f.write_str("a $num object")
            }
            fn visit_map<A: MapAccess<'de>>(self, mut a: A) -> Result<NumMap, A::Error> {
                let mut v = Vec::new();
                while let Some(k) = a.next_key::<String>()? {
                    let n = match k.as_str() {
                        "nan" => f64::NAN,
                        "inf" => f64::INFINITY,
                        "-inf" => f64::NEG_INFINITY,
                        "-0" => -0.0,
                        s => s
                            .parse::<f64>()
                            .map_err(|_| de::Error::custom(format!("bad $num key {s}")))?,
                    };
                    let JsonValue(x) = a.next_value()?;
                    v.push((n, x));
                }
                Ok(NumMap(v))
            }
        }
        d.deserialize_map(V)
    }
}

/// Parse one dump file's JSON text.
pub fn parse_json(text: &str) -> Result<DataValue, LoadError> {
    let mut d = serde_json::Deserializer::from_str(text);
    let JsonValue(v) = JsonValue::deserialize(&mut d).map_err(|e| LoadError::new(e.to_string()))?;
    d.end().map_err(|e| LoadError::new(e.to_string()))?;
    Ok(v)
}

// ---------------------------------------------------------------------------
// DumpTree: roots + reference index
// ---------------------------------------------------------------------------

/// The dump as trees: each root path (`data.skills`, `ConfigOptions`,
/// `files["Data/PearlSupports.lua"]`, …) with its value, plus the resolved
/// target of every `$ref` path.
#[derive(Debug, Default, Clone)]
pub struct DumpTree {
    pub roots: indexmap::IndexMap<AStr, DataValue>,
    pub index: HashMap<AStr, DataValue>,
}

/// One segment of a canonical path.
#[derive(Debug, Clone, PartialEq)]
enum Seg {
    Key(String),
    Num(f64),
    Meta,
    Other(String),
}

fn parse_path_segments(s: &str) -> Result<Vec<Seg>, LoadError> {
    let b = s.as_bytes();
    let mut i = 0;
    let mut out = Vec::new();
    let bad = || LoadError::new(format!("bad path syntax: {s}"));
    while i < b.len() {
        match b[i] {
            b'.' => {
                let st = i + 1;
                i = st;
                while i < b.len() && (b[i].is_ascii_alphanumeric() || b[i] == b'_') {
                    i += 1;
                }
                out.push(Seg::Key(s[st..i].to_owned()));
            }
            b'[' if b.get(i + 1) == Some(&b'"') => {
                // JSON string up to the matching quote
                let mut j = i + 2;
                while j < b.len() && b[j] != b'"' {
                    if b[j] == b'\\' {
                        j += 1;
                    }
                    j += 1;
                }
                let lit = &s[i + 1..=j];
                let key: String = serde_json::from_str(lit).map_err(|_| bad())?;
                out.push(Seg::Key(key));
                i = j + 1;
                if b.get(i) != Some(&b']') {
                    return Err(bad());
                }
                i += 1;
            }
            b'[' => {
                // number, true/false, @path or $bytes: up to the matching ']'
                let mut depth = 0;
                let mut j = i;
                loop {
                    match b.get(j) {
                        Some(b'[') => depth += 1,
                        Some(b']') => {
                            depth -= 1;
                            if depth == 0 {
                                break;
                            }
                        }
                        None => return Err(bad()),
                        _ => {}
                    }
                    j += 1;
                }
                let inner = &s[i + 1..j];
                match inner {
                    "nan" => out.push(Seg::Num(f64::NAN)),
                    "inf" => out.push(Seg::Num(f64::INFINITY)),
                    "-inf" => out.push(Seg::Num(f64::NEG_INFINITY)),
                    "-0" => out.push(Seg::Num(-0.0)),
                    x => match x.parse::<f64>() {
                        Ok(n) => out.push(Seg::Num(n)),
                        Err(_) => out.push(Seg::Other(x.to_owned())),
                    },
                }
                i = j + 1;
            }
            b'<' if s[i..].starts_with("<meta>") => {
                out.push(Seg::Meta);
                i += 6;
            }
            _ => return Err(bad()),
        }
    }
    Ok(out)
}

fn table_field<'a>(t: &'a DataTable, k: &str) -> Option<&'a DataValue> {
    // string keys are sorted bytewise in the dump
    match t
        .fields
        .binary_search_by(|(n, _)| n.as_bytes().cmp(k.as_bytes()))
    {
        Ok(i) => Some(&t.fields[i].1),
        Err(_) => t.fields.iter().find(|(n, _)| &**n == k).map(|(_, v)| v),
    }
}

impl DumpTree {
    /// Add a parsed file. `roots` are its manifest roots: one root holds the
    /// whole value; several roots (`misc.json`) are the members of an object
    /// `{ "<last path component>": value }`.
    pub fn add_file(&mut self, roots: &[String], v: DataValue) -> Result<(), LoadError> {
        if roots.len() == 1 {
            self.roots.insert(AStr::from(roots[0].as_str()), v);
            return Ok(());
        }
        let t = v
            .table()
            .ok_or_else(|| LoadError::new("multi-root file is not an object"))?
            .clone();
        for r in roots {
            let last = r.rsplit('.').next().unwrap_or(r);
            let m = table_field(&t, last)
                .ok_or_else(|| LoadError::new(format!("member {last} of root {r} missing")))?;
            self.roots.insert(AStr::from(r.as_str()), m.clone());
        }
        Ok(())
    }

    /// Resolve a canonical path by navigating from the longest matching root.
    pub fn navigate(&self, path: &str) -> Result<DataValue, LoadError> {
        let mut best: Option<(&AStr, &DataValue)> = None;
        for (r, v) in &self.roots {
            if path.starts_with(&**r)
                && matches!(
                    path.as_bytes().get(r.len()),
                    None | Some(b'.' | b'[' | b'<')
                )
            {
                if best.map_or(true, |(b, _)| b.len() < r.len()) {
                    best = Some((r, v));
                }
            }
        }
        let (root, v) = best.ok_or_else(|| LoadError::new(format!("no root for path {path}")))?;
        let mut cur = v.clone();
        for seg in parse_path_segments(&path[root.len()..])? {
            while let DataValue::Ref(p) = &cur {
                cur = self.navigate(p)?;
            }
            let next = match &cur {
                DataValue::Table(t) => Self::step(t, &seg).cloned(),
                _ => None,
            };
            cur = next.ok_or_else(|| LoadError::new(format!("path {path}: missing {seg:?}")))?;
        }
        Ok(cur)
    }

    fn step<'a>(t: &'a DataTable, seg: &Seg) -> Option<&'a DataValue> {
        match seg {
            Seg::Key(k) => table_field(t, k),
            Seg::Num(n) => {
                let v = t.index(*n);
                (!matches!(v, DataValue::Nil)).then_some(v)
            }
            Seg::Meta => t.meta.as_ref(),
            Seg::Other(_) => None,
        }
    }

    /// Collect every `$ref` target of the roots and resolve it into `index`.
    pub fn build_index(&mut self) -> Result<(), LoadError> {
        let mut targets = std::collections::BTreeSet::new();
        fn collect(v: &DataValue, out: &mut std::collections::BTreeSet<AStr>) {
            match v {
                DataValue::Ref(p) => {
                    out.insert(p.clone());
                }
                DataValue::Table(t) => {
                    for (_, x) in &t.fields {
                        collect(x, out);
                    }
                    for x in &t.seq {
                        collect(x, out);
                    }
                    for (_, x) in &t.num {
                        collect(x, out);
                    }
                    for (k, x) in &t.keys {
                        collect(k, out);
                        collect(x, out);
                    }
                    if let Some(m) = &t.meta {
                        collect(m, out);
                    }
                }
                _ => {}
            }
        }
        for v in self.roots.values() {
            collect(v, &mut targets);
        }
        self.add_targets(targets)
    }

    /// Resolve the given ref targets into `index`.
    pub fn add_targets(
        &mut self,
        targets: impl IntoIterator<Item = AStr>,
    ) -> Result<(), LoadError> {
        for p in targets {
            if self.index.contains_key(&p) {
                continue;
            }
            let v = self.navigate(&p)?;
            self.index.insert(p, v);
        }
        Ok(())
    }

    /// The target of a reference.
    pub fn resolve<'a>(&'a self, path: &str) -> Result<&'a DataValue, LoadError> {
        self.index
            .get(path)
            .ok_or_else(|| LoadError::new(format!("unresolved $ref {path}")))
    }

    /// Deserialise a root (or any canonical path) into `T`.
    pub fn get<T: for<'de> Deserialize<'de>>(&self, root: &str) -> Result<T, LoadError> {
        let v = match self.roots.get(root) {
            Some(v) => v.clone(),
            None => self.navigate(root)?,
        };
        T::deserialize(TreeDeserializer {
            v: &v,
            tree: self,
            path: &Path::Root(root),
        })
    }
}

// ---------------------------------------------------------------------------
// Binary form (data.bin): postcard
// ---------------------------------------------------------------------------

/// Format tag of data.bin.
pub const BIN_FORMAT: &str = "pob-gamedata-bin/1";

#[derive(serde::Serialize, Deserialize)]
struct BinDump {
    format: String,
    roots: Vec<(String, DataValue)>,
    targets: Vec<String>,
}

/// `data.bin` bytes for a tree (roots + the list of ref targets; the index is
/// re-resolved on load).
pub fn encode_bin(tree: &DumpTree) -> Result<Vec<u8>, LoadError> {
    let mut targets: Vec<String> = tree.index.keys().map(|k| k.to_string()).collect();
    targets.sort();
    let b = BinDump {
        format: BIN_FORMAT.into(),
        roots: tree
            .roots
            .iter()
            .map(|(k, v)| (k.to_string(), v.clone()))
            .collect(),
        targets,
    };
    postcard::to_allocvec(&b).map_err(|e| LoadError::new(format!("postcard encode: {e}")))
}

/// Decode `data.bin` bytes.
pub fn decode_bin(bytes: &[u8]) -> Result<DumpTree, LoadError> {
    let b: BinDump =
        postcard::from_bytes(bytes).map_err(|e| LoadError::new(format!("postcard decode: {e}")))?;
    if b.format != BIN_FORMAT {
        return Err(LoadError::new(format!(
            "data.bin format {} (expected {BIN_FORMAT})",
            b.format
        )));
    }
    let mut t = DumpTree::default();
    for (k, v) in b.roots {
        t.roots.insert(AStr::from(k), v);
    }
    t.add_targets(b.targets.into_iter().map(AStr::from))?;
    Ok(t)
}

// ---------------------------------------------------------------------------
// Shared Arcs by canonical path
// ---------------------------------------------------------------------------

/// Magic newtype name: [`TreeDeserializer`] answers it with the value's
/// canonical path (in [`CURRENT_KEY`]) so [`shared`] can reuse the `Arc`.
pub const SHARED_NEWTYPE: &str = "$pob::shared";

thread_local! {
    static CURRENT_KEY: RefCell<Option<String>> = const { RefCell::new(None) };
    static SHARED_CACHE: RefCell<HashMap<(TypeId, String), Arc<dyn Any + Send + Sync>>> = RefCell::new(HashMap::new());
}

/// Clear the shared-`Arc` cache (call before and after a load).
pub fn reset_shared_cache() {
    SHARED_CACHE.with(|c| c.borrow_mut().clear());
}

/// `#[serde(deserialize_with = "shared")]` for `Arc<T>` fields: one `Arc` per
/// Lua table (canonical path), so `data.gems[x].grantedEffect` and
/// `data.skills[y]` are the same object, as in Lua.
pub fn shared<'de, D, T>(d: D) -> Result<Arc<T>, D::Error>
where
    D: Deserializer<'de>,
    T: Deserialize<'de> + Send + Sync + 'static,
{
    struct V<T>(std::marker::PhantomData<T>);
    impl<'de, T: Deserialize<'de> + Send + Sync + 'static> Visitor<'de> for V<T> {
        type Value = Arc<T>;
        fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
            f.write_str("a shared table")
        }
        fn visit_newtype_struct<D: Deserializer<'de>>(self, d: D) -> Result<Arc<T>, D::Error> {
            let key = CURRENT_KEY.with(|k| k.borrow_mut().take());
            if let Some(key) = &key {
                let tid = TypeId::of::<T>();
                if let Some(hit) =
                    SHARED_CACHE.with(|c| c.borrow().get(&(tid, key.clone())).cloned())
                {
                    return Ok(hit.downcast::<T>().expect("shared cache type"));
                }
                let v = Arc::new(T::deserialize(d)?);
                SHARED_CACHE.with(|c| c.borrow_mut().insert((tid, key.clone()), v.clone()));
                return Ok(v);
            }
            Ok(Arc::new(T::deserialize(d)?))
        }
    }
    d.deserialize_newtype_struct(SHARED_NEWTYPE, V(std::marker::PhantomData))
}

/// `Option<Arc<T>>` variant of [`shared`].
pub fn shared_opt<'de, D, T>(d: D) -> Result<Option<Arc<T>>, D::Error>
where
    D: Deserializer<'de>,
    T: Deserialize<'de> + Send + Sync + 'static,
{
    shared(d).map(Some)
}

/// `Vec<Arc<T>>` of shared tables.
pub fn shared_vec<'de, D, T>(d: D) -> Result<Vec<Arc<T>>, D::Error>
where
    D: Deserializer<'de>,
    T: Deserialize<'de> + Send + Sync + 'static,
{
    struct Elem<T>(Arc<T>);
    impl<'de, T: Deserialize<'de> + Send + Sync + 'static> Deserialize<'de> for Elem<T> {
        fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
            shared(d).map(Elem)
        }
    }
    let v: Vec<Elem<T>> = Vec::deserialize(d)?;
    Ok(v.into_iter().map(|e| e.0).collect())
}

// ---------------------------------------------------------------------------
// TreeDeserializer
// ---------------------------------------------------------------------------

/// A serde deserializer over a [`DataValue`] (see the module docs).
#[derive(Clone, Copy)]
pub struct TreeDeserializer<'a, 'p> {
    pub v: &'a DataValue,
    pub tree: &'a DumpTree,
    pub path: &'p Path<'p>,
}

/// What a table presents as a map: real string keys, then the pseudo members.
enum Entry<'a> {
    Field(&'a str, &'a DataValue),
    Seq(&'a [DataValue]),
    Num(&'a [(f64, DataValue)]),
    Keys(&'a [(DataValue, DataValue)]),
    Meta(&'a DataValue),
}

fn table_entries(t: &DataTable) -> Vec<Entry<'_>> {
    let mut e: Vec<Entry<'_>> = t.fields.iter().map(|(k, v)| Entry::Field(k, v)).collect();
    if !t.seq.is_empty() {
        e.push(Entry::Seq(&t.seq));
    }
    if !t.num.is_empty() {
        e.push(Entry::Num(&t.num));
    }
    if !t.keys.is_empty() {
        e.push(Entry::Keys(&t.keys));
    }
    if let Some(m) = &t.meta {
        e.push(Entry::Meta(m));
    }
    e
}

/// A value the map/seq accessors hand out.
#[derive(Clone, Copy)]
enum Node<'a> {
    V(&'a DataValue),
    Seq(&'a [DataValue]),
    Num(&'a [(f64, DataValue)]),
    Keys(&'a [(DataValue, DataValue)]),
    Pair(&'a DataValue, &'a DataValue),
}

#[derive(Clone, Copy)]
struct NodeDe<'a, 'p> {
    n: Node<'a>,
    tree: &'a DumpTree,
    path: &'p Path<'p>,
}

impl<'a, 'p> TreeDeserializer<'a, 'p> {
    fn node(self) -> NodeDe<'a, 'p> {
        NodeDe {
            n: Node::V(self.v),
            tree: self.tree,
            path: self.path,
        }
    }
}

macro_rules! forward_to_node {
    ($($m:ident)*) => { $(
        fn $m<V: Visitor<'de>>(self, v: V) -> Result<V::Value, LoadError> { self.node().$m(v) }
    )* };
}

impl<'de, 'a, 'p> Deserializer<'de> for TreeDeserializer<'a, 'p> {
    type Error = LoadError;
    forward_to_node! {
        deserialize_any deserialize_bool deserialize_i8 deserialize_i16 deserialize_i32 deserialize_i64
        deserialize_u8 deserialize_u16 deserialize_u32 deserialize_u64 deserialize_f32 deserialize_f64
        deserialize_char deserialize_str deserialize_string deserialize_bytes deserialize_byte_buf
        deserialize_option deserialize_unit deserialize_seq deserialize_map deserialize_identifier
        deserialize_ignored_any
    }
    fn deserialize_unit_struct<V: Visitor<'de>>(
        self,
        n: &'static str,
        v: V,
    ) -> Result<V::Value, LoadError> {
        self.node().deserialize_unit_struct(n, v)
    }
    fn deserialize_newtype_struct<V: Visitor<'de>>(
        self,
        n: &'static str,
        v: V,
    ) -> Result<V::Value, LoadError> {
        self.node().deserialize_newtype_struct(n, v)
    }
    fn deserialize_tuple<V: Visitor<'de>>(self, l: usize, v: V) -> Result<V::Value, LoadError> {
        self.node().deserialize_tuple(l, v)
    }
    fn deserialize_tuple_struct<V: Visitor<'de>>(
        self,
        n: &'static str,
        l: usize,
        v: V,
    ) -> Result<V::Value, LoadError> {
        self.node().deserialize_tuple_struct(n, l, v)
    }
    fn deserialize_struct<V: Visitor<'de>>(
        self,
        n: &'static str,
        f: &'static [&'static str],
        v: V,
    ) -> Result<V::Value, LoadError> {
        self.node().deserialize_struct(n, f, v)
    }
    fn deserialize_enum<V: Visitor<'de>>(
        self,
        n: &'static str,
        f: &'static [&'static str],
        v: V,
    ) -> Result<V::Value, LoadError> {
        self.node().deserialize_enum(n, f, v)
    }
}

impl<'a, 'p> NodeDe<'a, 'p> {
    /// Follow references; returns the value and its canonical path (when it
    /// had to follow one).
    fn resolved(&self) -> Result<(Node<'a>, Option<&'a str>), LoadError> {
        let mut n = self.n;
        let mut p = None;
        let mut guard = 0;
        while let Node::V(DataValue::Ref(r)) = n {
            let t = self.tree.resolve(r).map_err(|e| at(e, self.path))?;
            p = Some(&**r);
            n = Node::V(t);
            guard += 1;
            if guard > 64 {
                return Err(at(LoadError::new("reference loop"), self.path));
            }
        }
        Ok((n, p))
    }
}

fn is_plain_int(x: f64) -> bool {
    x == x.trunc() && x.is_finite() && !(x == 0.0 && x.is_sign_negative()) && x.abs() < 9.2e18
}

struct MapAcc<'a, 'p> {
    entries: Vec<Entry<'a>>,
    i: usize,
    tree: &'a DumpTree,
    path: &'p Path<'p>,
}

impl<'de, 'a, 'p> MapAccess<'de> for MapAcc<'a, 'p> {
    type Error = LoadError;
    fn next_key_seed<K: DeserializeSeed<'de>>(
        &mut self,
        seed: K,
    ) -> Result<Option<K::Value>, LoadError> {
        let Some(e) = self.entries.get(self.i) else {
            return Ok(None);
        };
        let k = match e {
            Entry::Field(k, _) => *k,
            Entry::Seq(_) => "$seq",
            Entry::Num(_) => "$num",
            Entry::Keys(_) => "$keys",
            Entry::Meta(_) => "$meta",
        };
        let kp = Path::Key(self.path, k);
        seed.deserialize(k.into_deserializer())
            .map(Some)
            .map_err(|e: LoadError| at(e, &kp))
    }
    fn next_value_seed<V: DeserializeSeed<'de>>(&mut self, seed: V) -> Result<V::Value, LoadError> {
        let e = &self.entries[self.i];
        self.i += 1;
        match e {
            Entry::Field(k, v) => {
                let p = Path::Key(self.path, k);
                seed.deserialize(NodeDe {
                    n: Node::V(v),
                    tree: self.tree,
                    path: &p,
                })
                .map_err(|e| at(e, &p))
            }
            Entry::Seq(s) => {
                let p = Path::Pseudo(self.path, "$seq");
                seed.deserialize(NodeDe {
                    n: Node::Seq(s),
                    tree: self.tree,
                    path: &p,
                })
            }
            Entry::Num(s) => {
                let p = Path::Pseudo(self.path, "$num");
                seed.deserialize(NodeDe {
                    n: Node::Num(s),
                    tree: self.tree,
                    path: &p,
                })
            }
            Entry::Keys(s) => {
                let p = Path::Pseudo(self.path, "$keys");
                seed.deserialize(NodeDe {
                    n: Node::Keys(s),
                    tree: self.tree,
                    path: &p,
                })
            }
            Entry::Meta(m) => {
                let p = Path::Meta(self.path);
                seed.deserialize(NodeDe {
                    n: Node::V(m),
                    tree: self.tree,
                    path: &p,
                })
                .map_err(|e| at(e, &p))
            }
        }
    }
}

/// `$num` presented as a map with canonical number text keys.
struct NumAcc<'a, 'p> {
    items: &'a [(f64, DataValue)],
    i: usize,
    tree: &'a DumpTree,
    path: &'p Path<'p>,
}

impl<'de, 'a, 'p> MapAccess<'de> for NumAcc<'a, 'p> {
    type Error = LoadError;
    fn next_key_seed<K: DeserializeSeed<'de>>(
        &mut self,
        seed: K,
    ) -> Result<Option<K::Value>, LoadError> {
        let Some((n, _)) = self.items.get(self.i) else {
            return Ok(None);
        };
        let txt = num_text(*n);
        seed.deserialize(serde::de::value::StringDeserializer::<LoadError>::new(txt))
            .map(Some)
    }
    fn next_value_seed<V: DeserializeSeed<'de>>(&mut self, seed: V) -> Result<V::Value, LoadError> {
        let (n, v) = &self.items[self.i];
        self.i += 1;
        let p = Path::Num(self.path, *n);
        seed.deserialize(NodeDe {
            n: Node::V(v),
            tree: self.tree,
            path: &p,
        })
        .map_err(|e| at(e, &p))
    }
}

enum SeqItems<'a> {
    Vals(&'a [DataValue]),
    Pairs(&'a [(DataValue, DataValue)]),
    Two(&'a DataValue, &'a DataValue),
}

struct SeqAcc<'a, 'p> {
    items: SeqItems<'a>,
    i: usize,
    tree: &'a DumpTree,
    path: &'p Path<'p>,
}

impl<'de, 'a, 'p> SeqAccess<'de> for SeqAcc<'a, 'p> {
    type Error = LoadError;
    fn next_element_seed<T: DeserializeSeed<'de>>(
        &mut self,
        seed: T,
    ) -> Result<Option<T::Value>, LoadError> {
        let i = self.i;
        let n = match &self.items {
            SeqItems::Vals(s) => match s.get(i) {
                Some(v) => Node::V(v),
                None => return Ok(None),
            },
            SeqItems::Pairs(s) => match s.get(i) {
                Some((k, v)) => Node::Pair(k, v),
                None => return Ok(None),
            },
            SeqItems::Two(a, b) => match i {
                0 => Node::V(a),
                1 => Node::V(b),
                _ => return Ok(None),
            },
        };
        self.i += 1;
        let p = Path::Index(self.path, i + 1);
        seed.deserialize(NodeDe {
            n,
            tree: self.tree,
            path: &p,
        })
        .map(Some)
        .map_err(|e| at(e, &p))
    }
    fn size_hint(&self) -> Option<usize> {
        Some(
            match &self.items {
                SeqItems::Vals(s) => s.len(),
                SeqItems::Pairs(s) => s.len(),
                SeqItems::Two(..) => 2,
            } - self.i,
        )
    }
}

/// A function value presented as `{"$fn", "src", "hash"}`.
struct FnAcc<'a> {
    f: &'a LuaFnRef,
    i: usize,
}

impl<'de> MapAccess<'de> for FnAcc<'_> {
    type Error = LoadError;
    fn next_key_seed<K: DeserializeSeed<'de>>(
        &mut self,
        seed: K,
    ) -> Result<Option<K::Value>, LoadError> {
        let k = match self.i {
            0 => "$fn",
            1 => "src",
            2 => "hash",
            _ => return Ok(None),
        };
        seed.deserialize(k.into_deserializer()).map(Some)
    }
    fn next_value_seed<V: DeserializeSeed<'de>>(&mut self, seed: V) -> Result<V::Value, LoadError> {
        let s: &str = match self.i {
            0 => self.f.id.as_str(),
            1 => &self.f.src,
            _ => &self.f.hash,
        };
        self.i += 1;
        seed.deserialize(s.into_deserializer())
    }
}

impl<'de, 'a, 'p> Deserializer<'de> for NodeDe<'a, 'p> {
    type Error = LoadError;

    fn deserialize_any<V: Visitor<'de>>(self, v: V) -> Result<V::Value, LoadError> {
        let (n, _) = self.resolved()?;
        match n {
            Node::V(x) => match x {
                DataValue::Nil => v.visit_unit(),
                DataValue::Bool(b) => v.visit_bool(*b),
                DataValue::Num(x) => {
                    if is_plain_int(*x) {
                        v.visit_i64(*x as i64)
                    } else {
                        v.visit_f64(*x)
                    }
                }
                DataValue::Str(s) => v.visit_str(s),
                DataValue::Fn(f) => v.visit_map(FnAcc { f, i: 0 }),
                DataValue::Table(t) => {
                    if !t.seq.is_empty()
                        && t.fields.is_empty()
                        && t.num.is_empty()
                        && t.keys.is_empty()
                        && t.meta.is_none()
                    {
                        v.visit_seq(SeqAcc {
                            items: SeqItems::Vals(&t.seq),
                            i: 0,
                            tree: self.tree,
                            path: self.path,
                        })
                    } else {
                        v.visit_map(MapAcc {
                            entries: table_entries(t),
                            i: 0,
                            tree: self.tree,
                            path: self.path,
                        })
                    }
                }
                DataValue::Ref(_) => unreachable!("resolved"),
            },
            Node::Seq(s) => v.visit_seq(SeqAcc {
                items: SeqItems::Vals(s),
                i: 0,
                tree: self.tree,
                path: self.path,
            }),
            Node::Num(s) => v.visit_map(NumAcc {
                items: s,
                i: 0,
                tree: self.tree,
                path: self.path,
            }),
            Node::Keys(s) => v.visit_seq(SeqAcc {
                items: SeqItems::Pairs(s),
                i: 0,
                tree: self.tree,
                path: self.path,
            }),
            Node::Pair(a, b) => v.visit_seq(SeqAcc {
                items: SeqItems::Two(a, b),
                i: 0,
                tree: self.tree,
                path: self.path,
            }),
        }
    }

    fn deserialize_seq<V: Visitor<'de>>(self, v: V) -> Result<V::Value, LoadError> {
        let (n, _) = self.resolved()?;
        match n {
            Node::V(DataValue::Table(t)) => {
                if t.fields.is_empty() && t.num.is_empty() && t.keys.is_empty() && t.meta.is_none()
                {
                    v.visit_seq(SeqAcc {
                        items: SeqItems::Vals(&t.seq),
                        i: 0,
                        tree: self.tree,
                        path: self.path,
                    })
                } else {
                    let keys: Vec<String> = t
                        .fields
                        .iter()
                        .take(4)
                        .map(|(k, _)| k.to_string())
                        .collect();
                    Err(at(
                        LoadError::new(format!(
                            "expected a sequence, found a table with other keys ({keys:?}, {} numeric, meta {})",
                            t.num.len(),
                            t.meta.is_some()
                        )),
                        self.path,
                    ))
                }
            }
            _ => self.deserialize_any(v),
        }
    }

    fn deserialize_option<V: Visitor<'de>>(self, v: V) -> Result<V::Value, LoadError> {
        match self.n {
            Node::V(DataValue::Nil) => v.visit_none(),
            _ => v.visit_some(self),
        }
    }

    fn deserialize_newtype_struct<V: Visitor<'de>>(
        self,
        name: &'static str,
        v: V,
    ) -> Result<V::Value, LoadError> {
        if name == SHARED_NEWTYPE {
            let (n, via) = self.resolved()?;
            let key = match via {
                Some(p) => p.to_owned(),
                None => self.path.render(),
            };
            CURRENT_KEY.with(|k| *k.borrow_mut() = Some(key));
            let r = v.visit_newtype_struct(NodeDe {
                n,
                tree: self.tree,
                path: self.path,
            });
            CURRENT_KEY.with(|k| *k.borrow_mut() = None);
            return r;
        }
        v.visit_newtype_struct(self)
    }

    fn deserialize_enum<V: Visitor<'de>>(
        self,
        _n: &'static str,
        _f: &'static [&'static str],
        v: V,
    ) -> Result<V::Value, LoadError> {
        let (n, _) = self.resolved()?;
        match n {
            Node::V(DataValue::Str(s)) => v.visit_enum(s.as_ref().into_deserializer()),
            _ => Err(at(
                LoadError::new("expected a string naming an enum variant"),
                self.path,
            )),
        }
    }

    fn deserialize_struct<V: Visitor<'de>>(
        self,
        _n: &'static str,
        _f: &'static [&'static str],
        v: V,
    ) -> Result<V::Value, LoadError> {
        let (n, _) = self.resolved()?;
        match n {
            Node::V(DataValue::Table(t)) => v.visit_map(MapAcc {
                entries: table_entries(t),
                i: 0,
                tree: self.tree,
                path: self.path,
            }),
            Node::V(DataValue::Fn(f)) => v.visit_map(FnAcc { f, i: 0 }),
            Node::Num(s) => v.visit_map(NumAcc {
                items: s,
                i: 0,
                tree: self.tree,
                path: self.path,
            }),
            _ => Err(at(LoadError::new("expected a table"), self.path)),
        }
    }

    fn deserialize_map<V: Visitor<'de>>(self, v: V) -> Result<V::Value, LoadError> {
        let (n, _) = self.resolved()?;
        match n {
            Node::V(DataValue::Table(t)) => v.visit_map(MapAcc {
                entries: table_entries(t),
                i: 0,
                tree: self.tree,
                path: self.path,
            }),
            Node::Num(s) => v.visit_map(NumAcc {
                items: s,
                i: 0,
                tree: self.tree,
                path: self.path,
            }),
            _ => Err(at(LoadError::new("expected a table"), self.path)),
        }
    }

    /// Ignored values are not walked (and references not followed): a
    /// back-reference such as `statMap._grantedEffect` would recurse forever.
    fn deserialize_ignored_any<V: Visitor<'de>>(self, v: V) -> Result<V::Value, LoadError> {
        v.visit_unit()
    }

    serde::forward_to_deserialize_any! {
        bool i8 i16 i32 i64 i128 u8 u16 u32 u64 u128 f32 f64 char str string
        bytes byte_buf unit unit_struct tuple tuple_struct identifier
    }
}

// ---------------------------------------------------------------------------
// Field helpers for the dump shape
// ---------------------------------------------------------------------------

/// `{ a = true, b = true }` → set of names (values must be `true`).
pub fn name_set<'de, D: Deserializer<'de>>(d: D) -> Result<crate::NameSet, D::Error> {
    let m: indexmap::IndexMap<crate::Name, bool> = indexmap::IndexMap::deserialize(d)?;
    let mut s = ahash::AHashSet::new();
    for (k, v) in m {
        if !v {
            return Err(de::Error::custom(format!("set member {k} is false")));
        }
        s.insert(k);
    }
    Ok(Arc::new(s))
}

pub fn name_set_opt<'de, D: Deserializer<'de>>(d: D) -> Result<Option<crate::NameSet>, D::Error> {
    name_set(d).map(Some)
}

/// `{ [SkillType.X] = true, … }` (an object with `$num`, a `[true, …]` array
/// for keys 1..n, or `{}`) → [`SkillTypeSet`](crate::SkillTypeSet).
pub fn skill_type_set<'de, D: Deserializer<'de>>(d: D) -> Result<crate::SkillTypeSet, D::Error> {
    let g: SetShape = SetShape::deserialize(d)?;
    let mut s = crate::SkillTypeSet::default();
    for (k, v) in g.0 {
        if !v {
            return Err(de::Error::custom("skill type set member is false"));
        }
        if !(0.0..crate::SkillType::COUNT as f64).contains(&k) {
            return Err(de::Error::custom(format!("skill type {k} out of range")));
        }
        s.set(k as crate::SkillType::T, true);
    }
    Ok(s)
}

pub fn skill_type_set_opt<'de, D: Deserializer<'de>>(
    d: D,
) -> Result<Option<crate::SkillTypeSet>, D::Error> {
    skill_type_set(d).map(Some)
}

/// `{[number] = bool}` in any dump shape.
struct SetShape(Vec<(f64, bool)>);
impl<'de> Deserialize<'de> for SetShape {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        struct V;
        impl<'de> Visitor<'de> for V {
            type Value = SetShape;
            fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
                f.write_str("a set keyed by numbers")
            }
            fn visit_seq<A: SeqAccess<'de>>(self, mut a: A) -> Result<SetShape, A::Error> {
                let mut v = Vec::new();
                let mut i = 1.0;
                while let Some(b) = a.next_element::<bool>()? {
                    v.push((i, b));
                    i += 1.0;
                }
                Ok(SetShape(v))
            }
            fn visit_map<A: MapAccess<'de>>(self, mut a: A) -> Result<SetShape, A::Error> {
                let mut v = Vec::new();
                while let Some(k) = a.next_key::<String>()? {
                    match k.as_str() {
                        "$seq" => {
                            let s: Vec<bool> = a.next_value()?;
                            for (i, b) in s.into_iter().enumerate() {
                                v.push(((i + 1) as f64, b));
                            }
                        }
                        "$num" => {
                            let m: indexmap::IndexMap<String, bool> = a.next_value()?;
                            for (k, b) in m {
                                v.push((k.parse::<f64>().map_err(de::Error::custom)?, b));
                            }
                        }
                        other => {
                            return Err(de::Error::custom(format!(
                                "unexpected key {other} in a numeric set"
                            )))
                        }
                    }
                }
                Ok(SetShape(v))
            }
        }
        d.deserialize_any(V)
    }
}

/// A list of numbers given as a JSON array or `{}` (empty Lua table).
pub fn num_list<'de, D: Deserializer<'de>, T: Deserialize<'de>>(d: D) -> Result<Vec<T>, D::Error> {
    Vec::<T>::deserialize(d)
}

/// `{ "<key>" = number, … }` in dump order → `Vec<(Name, f64)>`.
pub fn name_num_pairs<'de, D: Deserializer<'de>>(
    d: D,
) -> Result<Vec<(crate::Name, f64)>, D::Error> {
    let m: indexmap::IndexMap<crate::Name, f64> = indexmap::IndexMap::deserialize(d)?;
    Ok(m.into_iter().collect())
}

/// `{ { "stat", value }, … }` → `Vec<(Name, f64)>` (qualityStats / constantStats).
pub fn stat_pairs<'de, D: Deserializer<'de>>(d: D) -> Result<Vec<(crate::Name, f64)>, D::Error> {
    let v: Vec<(crate::Name, f64)> = Vec::deserialize(d)?;
    Ok(v)
}

/// Any dumped value kept as a [`DataValue`] (for untyped members). References
/// are resolved one level (the result is self-contained only for trees
/// without further refs; nested refs stay [`DataValue::Ref`]).
pub fn data_value<'de, D: Deserializer<'de>>(d: D) -> Result<DataValue, D::Error> {
    Ok(AnyValue::deserialize(d)?.0)
}

/// [`DataValue`] rebuilt through any deserializer (used for untyped data members).
pub struct AnyValue(pub DataValue);
impl<'de> Deserialize<'de> for AnyValue {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        struct V;
        impl<'de> Visitor<'de> for V {
            type Value = AnyValue;
            fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
                f.write_str("any Lua value")
            }
            fn visit_unit<E>(self) -> Result<AnyValue, E> {
                Ok(AnyValue(DataValue::Nil))
            }
            fn visit_bool<E>(self, b: bool) -> Result<AnyValue, E> {
                Ok(AnyValue(DataValue::Bool(b)))
            }
            fn visit_i64<E>(self, n: i64) -> Result<AnyValue, E> {
                Ok(AnyValue(DataValue::Num(n as f64)))
            }
            fn visit_u64<E>(self, n: u64) -> Result<AnyValue, E> {
                Ok(AnyValue(DataValue::Num(n as f64)))
            }
            fn visit_f64<E>(self, n: f64) -> Result<AnyValue, E> {
                Ok(AnyValue(DataValue::Num(n)))
            }
            fn visit_str<E>(self, s: &str) -> Result<AnyValue, E> {
                Ok(AnyValue(DataValue::Str(AStr::from(s))))
            }
            fn visit_seq<A: SeqAccess<'de>>(self, mut a: A) -> Result<AnyValue, A::Error> {
                let mut seq = Vec::new();
                while let Some(AnyValue(x)) = a.next_element()? {
                    seq.push(x);
                }
                Ok(AnyValue(DataValue::Table(Arc::new(DataTable {
                    seq,
                    ..Default::default()
                }))))
            }
            fn visit_map<A: MapAccess<'de>>(self, mut a: A) -> Result<AnyValue, A::Error> {
                let mut t = DataTable::default();
                let mut f: Option<LuaFnRef> = None;
                while let Some(k) = a.next_key::<String>()? {
                    match k.as_str() {
                        "$fn" => {
                            f = Some(LuaFnRef {
                                id: a.next_value()?,
                                src: String::new(),
                                hash: String::new(),
                            });
                        }
                        "src" | "hash" if f.is_some() => {
                            let s: String = a.next_value()?;
                            if k == "src" {
                                f.as_mut().unwrap().src = s;
                            } else {
                                f.as_mut().unwrap().hash = s;
                            }
                        }
                        "$seq" => {
                            let s: Vec<AnyValue> = a.next_value()?;
                            t.seq = s.into_iter().map(|x| x.0).collect();
                        }
                        "$num" => {
                            let m: indexmap::IndexMap<String, AnyValue> = a.next_value()?;
                            for (k, v) in m {
                                let n = match k.as_str() {
                                    "-0" => -0.0,
                                    "inf" => f64::INFINITY,
                                    "-inf" => f64::NEG_INFINITY,
                                    "nan" => f64::NAN,
                                    s => s.parse().map_err(de::Error::custom)?,
                                };
                                t.num.push((n, v.0));
                            }
                        }
                        "$keys" => {
                            let l: Vec<(AnyValue, AnyValue)> = a.next_value()?;
                            t.keys = l.into_iter().map(|(k, v)| (k.0, v.0)).collect();
                        }
                        "$meta" => t.meta = Some(a.next_value::<AnyValue>()?.0),
                        _ => {
                            let v: AnyValue = a.next_value()?;
                            t.fields.push((AStr::from(k), v.0));
                        }
                    }
                }
                if let Some(f) = f {
                    return Ok(AnyValue(DataValue::Fn(Arc::new(f))));
                }
                Ok(AnyValue(DataValue::Table(Arc::new(t))))
            }
        }
        d.deserialize_any(V)
    }
}

/// A Lua table keyed by integers (`$seq` and/or `$num`, or a JSON array) →
/// `BTreeMap<key, T>`.
pub fn int_map<'de, D, T>(d: D) -> Result<std::collections::BTreeMap<i64, T>, D::Error>
where
    D: Deserializer<'de>,
    T: Deserialize<'de>,
{
    struct V<T>(std::marker::PhantomData<T>);
    impl<'de, T: Deserialize<'de>> Visitor<'de> for V<T> {
        type Value = std::collections::BTreeMap<i64, T>;
        fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
            f.write_str("an integer-keyed table")
        }
        fn visit_seq<A: SeqAccess<'de>>(self, mut a: A) -> Result<Self::Value, A::Error> {
            let mut m = std::collections::BTreeMap::new();
            let mut i = 1;
            while let Some(x) = a.next_element::<T>()? {
                m.insert(i, x);
                i += 1;
            }
            Ok(m)
        }
        fn visit_map<A: MapAccess<'de>>(self, mut a: A) -> Result<Self::Value, A::Error> {
            let mut m = std::collections::BTreeMap::new();
            while let Some(k) = a.next_key::<String>()? {
                match k.as_str() {
                    "$seq" => {
                        let s: Vec<T> = a.next_value()?;
                        for (i, x) in s.into_iter().enumerate() {
                            m.insert(i as i64 + 1, x);
                        }
                    }
                    "$num" => {
                        let n: indexmap::IndexMap<String, T> = a.next_value()?;
                        for (k, x) in n {
                            let i: i64 = k
                                .parse()
                                .map_err(|_| de::Error::custom(format!("non-integer key {k}")))?;
                            m.insert(i, x);
                        }
                    }
                    other => {
                        return Err(de::Error::custom(format!(
                            "unexpected key {other} in an integer-keyed table"
                        )))
                    }
                }
            }
            Ok(m)
        }
    }
    d.deserialize_any(V(std::marker::PhantomData))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn paths_and_numbers() {
        assert_eq!(num_text(10.0), "10");
        assert_eq!(num_text(0.1), "0.1");
        assert_eq!(num_text(-0.0), "-0");
        let mut s = String::new();
        str_key_path("end", &mut s);
        str_key_path("Fireball", &mut s);
        str_key_path("Metadata/Items/Gems/X", &mut s);
        assert_eq!(s, r#"["end"].Fireball["Metadata/Items/Gems/X"]"#);
        let segs =
            parse_path_segments(r#".skills.Fireball.levels[3]["a b"]<meta>.__index"#).unwrap();
        assert_eq!(segs.len(), 7);
        assert_eq!(segs[3], Seg::Num(3.0));
        assert_eq!(segs[4], Seg::Key("a b".into()));
    }

    #[test]
    fn tree_refs_and_sharing() {
        let mut t = DumpTree::default();
        t.add_file(
            &["data.skills".into()],
            parse_json(r#"{"A":{"id":"A","n":1},"B":{"id":"B","n":2}}"#).unwrap(),
        )
        .unwrap();
        t.add_file(
            &["data.gems".into()],
            parse_json(
                r#"{"g1":{"s":{"$ref":"data.skills.A"}},"g2":{"s":{"$ref":"data.skills.A"}}}"#,
            )
            .unwrap(),
        )
        .unwrap();
        t.build_index().unwrap();
        #[derive(Deserialize, Debug)]
        #[serde(deny_unknown_fields)]
        struct Skill {
            id: String,
            n: f64,
        }
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct Gem {
            #[serde(deserialize_with = "shared")]
            s: Arc<Skill>,
        }
        reset_shared_cache();
        struct Sk(Arc<Skill>);
        impl<'de> Deserialize<'de> for Sk {
            fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
                shared(d).map(Sk)
            }
        }
        let skills: indexmap::IndexMap<String, Sk> = t.get("data.skills").unwrap();
        let gems: indexmap::IndexMap<String, Gem> = t.get("data.gems").unwrap();
        assert!(Arc::ptr_eq(&gems["g1"].s, &gems["g2"].s));
        assert!(Arc::ptr_eq(&gems["g1"].s, &skills["A"].0));
        assert_eq!(skills["B"].0.n, 2.0);
        assert_eq!(skills["B"].0.id, "B");
        // unknown key error carries the path
        let bad = parse_json(r#"{"X":{"id":"X","n":1,"zzz":true}}"#).unwrap();
        let mut t2 = DumpTree::default();
        t2.add_file(&["data.skills".into()], bad).unwrap();
        let e = t2
            .get::<indexmap::IndexMap<String, Skill>>("data.skills")
            .unwrap_err();
        assert!(e.to_string().contains("data.skills.X.zzz"), "{e}");
        // binary round trip
        let bytes = encode_bin(&t).unwrap();
        let t3 = decode_bin(&bytes).unwrap();
        assert_eq!(t3.roots.len(), 2);
        assert!(t3.index.contains_key("data.skills.A"));
    }
}
