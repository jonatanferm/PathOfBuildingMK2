//! `table.sort` (calc doc §4.1 `sort.rs`): exact port of LuaJIT 2.1
//! `lib_table.c` `auxsort` (the Lua 5.1 quicksort with median-of-three).
//!
//! It is not stable: for comparators with ties the resulting permutation is
//! the one LuaJIT produces, which this port reproduces comparison for
//! comparison (same calls, same argument order, same swaps). An inconsistent
//! comparator raises "invalid order function for sorting" at the same point
//! LuaJIT does, leaving the slice in the same partially sorted state.
//!
//! Lua's `t[1..n]` is `v[0..n]`. The pivot `P` that `auxsort` keeps on the Lua
//! stack always equals `v[u-1]` while it is compared (the partition loop never
//! writes index `u-1`), so it is read from there instead of being cloned.

/// `table.sort(t, comp)` where `lt(a, b)` is `comp(a, b)` ("a sorts before b").
pub fn sort<T>(v: &mut [T], mut lt: impl FnMut(&T, &T) -> bool) {
    let n = v.len() as i64;
    auxsort(v, &mut lt, 1, n);
}

/// `table.sort(t)` without a comparator (`lua_lessthan`: `<`).
pub fn sort_default<T: PartialOrd>(v: &mut [T]) {
    sort(v, |a, b| a < b);
}

#[inline]
fn ix(i: i64) -> usize {
    (i - 1) as usize
}

fn auxsort<T>(a: &mut [T], lt: &mut impl FnMut(&T, &T) -> bool, mut l: i64, mut u: i64) {
    while l < u {
        // for tail recursion
        // sort elements a[l], a[(l+u)/2] and a[u]
        if lt(&a[ix(u)], &a[ix(l)]) {
            // a[u] < a[l]?
            a.swap(ix(l), ix(u)); // swap a[l] - a[u]
        }
        if u - l == 1 {
            break; // only 2 elements
        }
        let mut i = (l + u) / 2;
        if lt(&a[ix(i)], &a[ix(l)]) {
            // a[i]<a[l]?
            a.swap(ix(i), ix(l));
        } else if lt(&a[ix(u)], &a[ix(i)]) {
            // a[u]<a[i]?
            a.swap(ix(i), ix(u));
        }
        if u - l == 2 {
            break; // only 3 elements
        }
        // Pivot P = a[i]; swap a[i] with a[u-1].
        a.swap(ix(i), ix(u - 1));
        let p = ix(u - 1);
        // a[l] <= P == a[u-1] <= a[u], only need to sort from l+1 to u-2
        i = l;
        let mut j = u - 1;
        loop {
            // invariant: a[l..i] <= P <= a[j..u]
            // repeat ++i until a[i] >= P
            loop {
                i += 1;
                if !lt(&a[ix(i)], &a[p]) {
                    break;
                }
                if i >= u {
                    crate::lua_error!("invalid order function for sorting");
                }
            }
            // repeat --j until a[j] <= P
            loop {
                j -= 1;
                if !lt(&a[p], &a[ix(j)]) {
                    break;
                }
                if j <= l {
                    crate::lua_error!("invalid order function for sorting");
                }
            }
            if j < i {
                break;
            }
            a.swap(ix(i), ix(j));
        }
        a.swap(ix(u - 1), ix(i)); // swap pivot (a[u-1]) with a[i]
                                  // a[l..i-1] <= a[i] == P <= a[i+1..u]
                                  // adjust so that smaller half is in [j..i] and larger one in [l..u]
        if i - l < u - i {
            j = l;
            i -= 1;
            l = i + 2;
        } else {
            j = i + 1;
            i = u;
            u = j - 2;
        }
        auxsort(a, lt, j, i); // call recursively the smaller one
    } // repeat the routine for the larger one
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sorts() {
        let mut v = vec![5, 3, 9, 1, 1, 7, 2, 8, 0, 6];
        sort(&mut v, |a, b| a < b);
        assert_eq!(v, vec![0, 1, 1, 2, 3, 5, 6, 7, 8, 9]);
        let mut w = vec![3.0, 1.0, 2.0];
        sort_default(&mut w);
        assert_eq!(w, vec![1.0, 2.0, 3.0]);
        let mut e: Vec<i32> = vec![];
        sort(&mut e, |a, b| a < b);
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn invalid_order_function() {
        let r = crate::error::catch(|| {
            let mut v = vec![1; 10];
            sort(&mut v, |a, b| a <= b);
        });
        assert_eq!(r.unwrap_err().0, "invalid order function for sorting");
    }
}
