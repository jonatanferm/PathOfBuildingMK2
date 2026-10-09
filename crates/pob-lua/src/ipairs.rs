//! `for i, v in ipairs(t)` (calc doc §3.2, §4.1 `ipairs.rs`).
//!
//! Lua's `ipairs` reads `t[i]` afresh on every step and stops at the first
//! `nil`, so elements appended or removed by the loop body are seen exactly as
//! in Lua. [`for_ipairs!`](crate::for_ipairs) does the same over any container
//! implementing [`IpairsGet`].

use std::rc::Rc;

use crate::field::Shared;

/// Live 1-based read `t[i]` for `ipairs`.
pub trait IpairsGet {
    type Item;
    fn ipairs_get(&self, i1: usize) -> Option<Self::Item>;
}

impl<T: Clone> IpairsGet for Shared<Vec<T>> {
    type Item = T;
    fn ipairs_get(&self, i1: usize) -> Option<T> {
        self.get(i1)
    }
}

impl<T: Clone> IpairsGet for [T] {
    type Item = T;
    fn ipairs_get(&self, i1: usize) -> Option<T> {
        if i1 >= 1 {
            self.get(i1 - 1).cloned()
        } else {
            None
        }
    }
}

impl<T: Clone> IpairsGet for Vec<T> {
    type Item = T;
    fn ipairs_get(&self, i1: usize) -> Option<T> {
        self.as_slice().ipairs_get(i1)
    }
}

impl<T: Clone> IpairsGet for crate::ljtab::LjIntMap<T> {
    type Item = T;
    fn ipairs_get(&self, i1: usize) -> Option<T> {
        self.get(i1 as i64)
    }
}

impl<C: IpairsGet + ?Sized> IpairsGet for &C {
    type Item = C::Item;
    fn ipairs_get(&self, i1: usize) -> Option<C::Item> {
        (**self).ipairs_get(i1)
    }
}

impl<C: IpairsGet + ?Sized> IpairsGet for Rc<C> {
    type Item = C::Item;
    fn ipairs_get(&self, i1: usize) -> Option<C::Item> {
        (**self).ipairs_get(i1)
    }
}

/// `for_ipairs!((i, v) in list => { body })`: reads `list[i]` live each step
/// and stops at the first nil. `break` and `continue` work as in Lua loops
/// (`goto continue` → `continue`). `i` is the 1-based `usize` index.
#[macro_export]
macro_rules! for_ipairs {
    (($i:pat, $v:pat) in $t:expr => $body:block) => {{
        let __pob_t = &$t;
        let mut __pob_i: usize = 0;
        loop {
            __pob_i += 1;
            let ::std::option::Option::Some(__pob_v) =
                $crate::ipairs::IpairsGet::ipairs_get(__pob_t, __pob_i)
            else {
                break;
            };
            #[allow(unused_variables)]
            let $i = __pob_i;
            let $v = __pob_v;
            $body
        }
    }};
}

/// `ipairs` over a `Vec<Option<T>>` whose body may punch holes
/// (`list[i] = nil`) or otherwise mutate the list (createActiveSkill
/// `rejectedSupportsIndices`, CalcActiveSkill.lua:49-54). `f(i, v, list)` gets
/// the 1-based index, the current value and the list itself; iteration reads
/// `list[i]` live and stops at the first `None` or the end.
pub fn ipairs_live<T: Clone>(
    v: &mut Vec<Option<T>>,
    mut f: impl FnMut(usize, T, &mut Vec<Option<T>>),
) {
    let mut i = 1usize;
    loop {
        let Some(Some(x)) = v.get(i - 1).cloned() else {
            break;
        };
        f(i, x, v);
        i += 1;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn live_reads() {
        let list = Shared::new(vec![1, 2, 3]);
        let mut seen = vec![];
        for_ipairs!((i, v) in list => {
            if i == 1 {
                list.push(4); // appended during the loop: visited, as in Lua
            }
            if v == 2 {
                continue;
            }
            seen.push((i, v));
        });
        assert_eq!(seen, vec![(1, 1), (3, 3), (4, 4)]);

        let plain = vec![10, 20];
        let mut sum = 0;
        for_ipairs!((_, v) in plain => { sum += v; });
        assert_eq!(sum, 30);
    }

    #[test]
    fn live_holes() {
        let mut v = vec![Some(1), Some(2), Some(3), Some(4)];
        let mut seen = vec![];
        ipairs_live(&mut v, |i, x, list| {
            seen.push(x);
            if i == 2 {
                list[2] = None; // punch the next element: iteration stops there
            }
        });
        assert_eq!(seen, vec![1, 2]);
    }
}
