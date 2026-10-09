//! The aliasing model for fixed-shape Lua tables (calc doc D2, §3.3).
//!
//! * [`Field<T>`]: a reassignable field of an aliased Lua table. It never
//!   hands out references, so no borrow can be held across a call.
//! * [`Shared<T>`]: a Lua table with a fixed Rust shape that is aliased and
//!   mutated in place (lists, maps).
//! * [`PtrSet`] / [`PtrMap`]: Lua tables keyed by table identity, iterated in
//!   insertion order (PoB's own order is address-hashed, i.e. random; §3.4).

use std::cell::RefCell;
use std::rc::Rc;

/// Marker: cloning is O(1) and allocation-free.
pub trait CheapClone: Clone {}
impl CheapClone for f64 {}
impl CheapClone for bool {}
impl CheapClone for u32 {}
impl CheapClone for i64 {}
impl CheapClone for usize {}
impl CheapClone for crate::Name {}
impl<T: ?Sized> CheapClone for std::rc::Rc<T> {}
impl<T: ?Sized> CheapClone for std::sync::Arc<T> {}
impl<T> CheapClone for std::rc::Weak<T> {}
impl<T: CheapClone> CheapClone for Option<T> {}
impl<T> CheapClone for Shared<T> {}

/// A reassignable field of an aliased Lua table. Never hands out references.
#[derive(Default)]
pub struct Field<T: CheapClone>(RefCell<T>);

impl<T: CheapClone> Field<T> {
    pub fn new(v: T) -> Self {
        Self(RefCell::new(v))
    }
    #[inline]
    pub fn get(&self) -> T {
        self.0.borrow().clone()
    }
    #[inline]
    pub fn set(&self, v: T) {
        *self.0.borrow_mut() = v;
    }
    #[inline]
    pub fn replace(&self, v: T) -> T {
        self.0.replace(v)
    }
}

impl<T: CheapClone + Default> Field<T> {
    pub fn take(&self) -> T {
        self.0.take()
    }
}

impl<T: CheapClone + std::fmt::Debug> std::fmt::Debug for Field<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_tuple("Field").field(&self.get()).finish()
    }
}

impl<T: CheapClone> Clone for Field<T> {
    /// A copy of the current value in a new, unaliased field.
    fn clone(&self) -> Self {
        Self::new(self.get())
    }
}

const LEAF_RULE: &str =
    "re-entrant borrow of a Shared value: closures passed to Shared::with/with_mut \
                         must be leaf code (calc doc §3.3 rule 1)";

/// A Lua table with a fixed Rust shape that is aliased and mutated in place (lists, maps).
pub struct Shared<T>(Rc<RefCell<T>>);

impl<T> Clone for Shared<T> {
    fn clone(&self) -> Self {
        Self(self.0.clone())
    }
}

impl<T: Default> Default for Shared<T> {
    fn default() -> Self {
        Self::new(T::default())
    }
}

impl<T: std::fmt::Debug> std::fmt::Debug for Shared<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self.0.try_borrow() {
            Ok(v) => f.debug_tuple("Shared").field(&*v).finish(),
            Err(_) => f.write_str("Shared(<borrowed>)"),
        }
    }
}

impl<T> Shared<T> {
    pub fn new(v: T) -> Self {
        Self(Rc::new(RefCell::new(v)))
    }

    /// Lua `rawequal(a, b)`: the same table.
    pub fn ptr_eq(&self, o: &Self) -> bool {
        Rc::ptr_eq(&self.0, &o.0)
    }

    /// Leaf-only closures (§3.3).
    pub fn with<R>(&self, f: impl FnOnce(&T) -> R) -> R {
        let g = self
            .0
            .try_borrow()
            .unwrap_or_else(|_| panic!("{LEAF_RULE}"));
        f(&g)
    }

    /// Leaf-only closures (§3.3).
    pub fn with_mut<R>(&self, f: impl FnOnce(&mut T) -> R) -> R {
        let mut g = self
            .0
            .try_borrow_mut()
            .unwrap_or_else(|_| panic!("{LEAF_RULE}"));
        f(&mut g)
    }
}

impl<T: Clone> Shared<Vec<T>> {
    /// `#t` (hole-free list).
    pub fn len(&self) -> usize {
        self.with(Vec::len)
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Lua `t[i]`, 1-based. `None` is Lua `nil`.
    pub fn get(&self, i1: usize) -> Option<T> {
        self.with(|v| {
            if i1 >= 1 {
                v.get(i1 - 1).cloned()
            } else {
                None
            }
        })
    }

    /// `t_insert(t, v)`.
    pub fn push(&self, v: T) {
        self.with_mut(|vec| vec.push(v));
    }

    /// `t_insert(t, i, v)` (LuaJIT lib_table.c `table_insert`): shifts
    /// `t[i..#t]` up by one. A position outside `1..=#t+1` would make a hole,
    /// which a hole-free list cannot represent.
    pub fn insert(&self, i1: usize, v: T) {
        self.with_mut(|vec| {
            if i1 >= 1 && i1 <= vec.len() + 1 {
                vec.insert(i1 - 1, v);
                return;
            }
            let n = vec.len();
            crate::lua_error!("table.insert at position {i1} of a list of length {n} would create a hole (hole-free list contract, calc doc §3.2)")
        });
    }

    /// `t_remove(t)` / `t_remove(t, i)` (LuaJIT lib_table.c `table_remove`):
    /// returns the removed value; a position outside `1..=#t` returns `nil`
    /// and changes nothing.
    pub fn remove(&self, i1: Option<usize>) -> Option<T> {
        self.with_mut(|vec| match i1 {
            None => vec.pop(),
            Some(pos) if pos >= 1 && pos <= vec.len() => Some(vec.remove(pos - 1)),
            Some(_) => None,
        })
    }

    /// `t[i] = v` for `i` in `1..=#t+1`.
    pub fn set(&self, i1: usize, v: T) {
        self.with_mut(|vec| {
            if i1 >= 1 && i1 <= vec.len() {
                vec[i1 - 1] = v;
            } else if i1 == vec.len() + 1 {
                vec.push(v);
            } else {
                let n = vec.len();
                crate::lua_error!("t[{i1}] = v on a list of length {n} would create a hole (hole-free list contract, calc doc §3.2)")
            }
        });
    }

    /// `wipeTable(t)`.
    pub fn wipe(&self) {
        self.with_mut(Vec::clear);
    }

    /// A copy of the current elements (for `ipairs` with a snapshot).
    pub fn snapshot(&self) -> Vec<T> {
        self.with(Clone::clone)
    }
}

/// Lua tables keyed by table identity (`{[srcInstance] = true}`), iterated in
/// insertion order.
pub struct PtrSet<T>(Vec<Rc<T>>);

impl<T> Default for PtrSet<T> {
    fn default() -> Self {
        Self(Vec::new())
    }
}

impl<T> Clone for PtrSet<T> {
    fn clone(&self) -> Self {
        Self(self.0.clone())
    }
}

impl<T> PtrSet<T> {
    pub fn new() -> Self {
        Self::default()
    }
    /// `t[k] = true`; returns whether `k` was absent.
    pub fn insert(&mut self, k: &Rc<T>) -> bool {
        if self.contains(k) {
            false
        } else {
            self.0.push(k.clone());
            true
        }
    }
    /// `t[k] ~= nil`.
    pub fn contains(&self, k: &Rc<T>) -> bool {
        self.0.iter().any(|x| Rc::ptr_eq(x, k))
    }
    /// `t[k] = nil`; returns whether `k` was present.
    pub fn remove(&mut self, k: &Rc<T>) -> bool {
        match self.0.iter().position(|x| Rc::ptr_eq(x, k)) {
            Some(i) => {
                self.0.remove(i);
                true
            }
            None => false,
        }
    }
    pub fn len(&self) -> usize {
        self.0.len()
    }
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
    /// `pairs(t)` keys, snapshot in insertion order.
    pub fn keys(&self) -> Vec<Rc<T>> {
        self.0.clone()
    }
    /// `wipeTable(t)`.
    pub fn wipe(&mut self) {
        self.0.clear();
    }
}

/// Lua tables keyed by table identity with a value, iterated in insertion order.
pub struct PtrMap<K, V>(Vec<(Rc<K>, V)>);

impl<K, V> Default for PtrMap<K, V> {
    fn default() -> Self {
        Self(Vec::new())
    }
}

impl<K, V: Clone> Clone for PtrMap<K, V> {
    fn clone(&self) -> Self {
        Self(self.0.clone())
    }
}

impl<K, V: Clone> PtrMap<K, V> {
    pub fn new() -> Self {
        Self::default()
    }
    /// `t[k]`.
    pub fn get(&self, k: &Rc<K>) -> Option<V> {
        self.0
            .iter()
            .find(|(x, _)| Rc::ptr_eq(x, k))
            .map(|(_, v)| v.clone())
    }
    /// `t[k] = v` (`None` = assign nil, which removes the key).
    pub fn set(&mut self, k: &Rc<K>, v: Option<V>) {
        let pos = self.0.iter().position(|(x, _)| Rc::ptr_eq(x, k));
        match (pos, v) {
            (Some(i), Some(v)) => self.0[i].1 = v,
            (Some(i), None) => {
                self.0.remove(i);
            }
            (None, Some(v)) => self.0.push((k.clone(), v)),
            (None, None) => {}
        }
    }
    pub fn contains_key(&self, k: &Rc<K>) -> bool {
        self.0.iter().any(|(x, _)| Rc::ptr_eq(x, k))
    }
    pub fn len(&self) -> usize {
        self.0.len()
    }
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
    /// `pairs(t)`, snapshot in insertion order.
    pub fn pairs(&self) -> Vec<(Rc<K>, V)> {
        self.0.clone()
    }
    /// `wipeTable(t)`.
    pub fn wipe(&mut self) {
        self.0.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn field_and_shared() {
        let f = Field::new(1.5);
        assert_eq!(f.get(), 1.5);
        f.set(2.0);
        assert_eq!(f.replace(3.0), 2.0);
        let g: Field<Option<Rc<str>>> = Field::default();
        assert!(g.take().is_none());

        let s = Shared::new(vec![1, 2, 3]);
        let alias = s.clone();
        assert!(s.ptr_eq(&alias));
        alias.push(4);
        assert_eq!(s.len(), 4);
        assert_eq!(s.get(1), Some(1));
        assert_eq!(s.get(0), None);
        assert_eq!(s.get(5), None);
        s.insert(1, 0);
        assert_eq!(s.snapshot(), vec![0, 1, 2, 3, 4]);
        s.insert(6, 5);
        assert_eq!(s.remove(Some(1)), Some(0));
        assert_eq!(s.remove(Some(9)), None);
        assert_eq!(s.remove(None), Some(5));
        s.set(4, 9);
        s.set(5, 10);
        assert_eq!(s.snapshot(), vec![1, 2, 3, 9, 10]);
        s.wipe();
        assert!(s.is_empty());
    }

    #[test]
    fn ptr_tables() {
        let a = Rc::new(1);
        let b = Rc::new(1);
        let mut set = PtrSet::new();
        assert!(set.insert(&b));
        assert!(set.insert(&a));
        assert!(!set.insert(&a));
        assert!(set.contains(&a));
        assert!(Rc::ptr_eq(&set.keys()[0], &b));
        assert!(set.remove(&b));
        assert_eq!(set.len(), 1);
        let mut map = PtrMap::new();
        map.set(&a, Some("x"));
        map.set(&b, Some("y"));
        map.set(&a, None);
        assert_eq!(map.get(&b), Some("y"));
        assert_eq!(map.get(&a), None);
        assert_eq!(map.len(), 1);
    }

    #[test]
    #[should_panic(expected = "§3.3")]
    fn reentrant_borrow_cites_rule() {
        let s = Shared::new(vec![1]);
        s.with(|_| s.push(2));
    }
}
