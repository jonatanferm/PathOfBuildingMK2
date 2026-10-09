//! Interned Lua string keys (calc doc §4.1 `name.rs`, D14).
//!
//! `Name` is an interned, `Copy` string with a precomputed hash. `n!("Life")`
//! interns once per call site; `cat!(a, "Min")` is Lua's `a .. "Min"` producing
//! a `Name` (numbers concatenate with `tostring`, i.e. `%.14g`).

use std::cell::RefCell;
use std::collections::HashMap;

use smallvec::SmallVec;

/// Interned, `Copy`, precomputed hash, `Send + Sync`.
pub type Name = ustr::Ustr;

/// Literal key; one intern per call site (OnceLock), then a plain load.
#[macro_export]
macro_rules! n {
    ($s:literal) => {{
        static N: ::std::sync::OnceLock<$crate::Name> = ::std::sync::OnceLock::new();
        *N.get_or_init(|| $crate::Name::from($s))
    }};
}

/// `"A", "B"` query name lists.
#[macro_export]
macro_rules! nm { ($($s:literal),* $(,)?) => { &[$($crate::n!($s)),*][..] }; }

/// Lua `a .. b .. c` producing an interned Name. Numbers use tostring (%.14g).
/// Formats into a 128-byte stack buffer (heap only beyond that), then calls
/// [`CatBuf::intern`], which consults a thread-local memo before touching the
/// global interner.
#[macro_export]
macro_rules! cat {
    ($($p:expr),+ $(,)?) => {{ let mut b = $crate::name::CatBuf::new();
        $( $crate::format::LuaConcat::concat_into(&$p, &mut b); )+ b.intern() }};
}

/// Byte buffer for `cat!`. Only `&str` and ASCII number text are ever pushed,
/// so the content is always valid UTF-8.
pub struct CatBuf {
    buf: SmallVec<[u8; 128]>,
}

impl Default for CatBuf {
    fn default() -> Self {
        Self::new()
    }
}

thread_local! {
    /// Content-keyed memo in front of the global interner. Keyed on the
    /// concatenated bytes (FNV-1a), not on part pointers: a pointer key is
    /// only sound for `'static`/interned parts, and `cat!` also takes
    /// transient `String`s.
    static CAT_MEMO: RefCell<HashMap<u64, SmallVec<[Name; 1]>>> = RefCell::new(HashMap::new());
    static CONDITION_MEMO: RefCell<ustr::UstrMap<Name>> = RefCell::new(ustr::UstrMap::default());
    static MULTIPLIER_MEMO: RefCell<ustr::UstrMap<Name>> = RefCell::new(ustr::UstrMap::default());
}

#[inline]
fn fnv1a(bytes: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for &b in bytes {
        h ^= u64::from(b);
        h = h.wrapping_mul(0x0100_0000_01b3);
    }
    h
}

impl CatBuf {
    #[inline]
    pub fn new() -> Self {
        Self {
            buf: SmallVec::new(),
        }
    }

    #[inline]
    pub fn push_str(&mut self, s: &str) {
        self.buf.extend_from_slice(s.as_bytes());
    }

    /// The text accumulated so far.
    pub fn as_str(&self) -> &str {
        std::str::from_utf8(&self.buf).expect("CatBuf holds only UTF-8")
    }

    /// Intern the concatenation.
    pub fn intern(self) -> Name {
        let key = fnv1a(&self.buf);
        let hit = CAT_MEMO.with(|m| {
            m.borrow().get(&key).and_then(|v| {
                v.iter()
                    .copied()
                    .find(|n| n.as_str().as_bytes() == &self.buf[..])
            })
        });
        if let Some(n) = hit {
            return n;
        }
        let n = Name::from(self.as_str());
        CAT_MEMO.with(|m| m.borrow_mut().entry(key).or_default().push(n));
        n
    }
}

/// Lua drops nil names in ModStore varargs: `names([Some(a), cond.then(|| b)])`.
pub fn names<const N: usize>(xs: [Option<Name>; N]) -> SmallVec<[Name; 4]> {
    xs.into_iter().flatten().collect()
}

/// `"Condition:"..var`: the ModStore.lua 20-28 memo table.
pub fn conditionName(var: Name) -> Name {
    if let Some(n) = CONDITION_MEMO.with(|m| m.borrow().get(&var).copied()) {
        return n;
    }
    let n = cat!("Condition:", var);
    CONDITION_MEMO.with(|m| m.borrow_mut().insert(var, n));
    n
}

/// `"Multiplier:"..var`: the ModStore.lua 20-28 memo table.
pub fn multiplierName(var: Name) -> Name {
    if let Some(n) = MULTIPLIER_MEMO.with(|m| m.borrow().get(&var).copied()) {
        return n;
    }
    let n = cat!("Multiplier:", var);
    MULTIPLIER_MEMO.with(|m| m.borrow_mut().insert(var, n));
    n
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn macros() {
        let a = n!("Life");
        assert_eq!(a.as_str(), "Life");
        assert_eq!(a, Name::from("Life"));
        let l = nm!["A", "B"];
        assert_eq!(l.len(), 2);
        assert_eq!(l[1].as_str(), "B");
        let dt = "Physical";
        assert_eq!(cat!(dt, "Min").as_str(), "PhysicalMin");
        assert_eq!(
            cat!("x", 1.0, "y", 0.1, 1e15, -0.0).as_str(),
            "x1y0.11e+15-0"
        );
        assert_eq!(cat!(String::from("a"), 7_i64), Name::from("a7"));
        assert_eq!(conditionName(n!("Foo")).as_str(), "Condition:Foo");
        assert_eq!(multiplierName(n!("Foo")).as_str(), "Multiplier:Foo");
        let v = names([Some(n!("A")), None, Some(n!("C"))]);
        assert_eq!(v.as_slice(), &[n!("A"), n!("C")]);
        // Long concatenation (heap path).
        let long = "z".repeat(300);
        assert_eq!(cat!(long.as_str(), "!").as_str().len(), 301);
    }
}
