//! L0 (calc doc §9.4): `pob_lua::sort` against LuaJIT `table.sort`: the
//! resulting permutation for tie-heavy, reversed, total, inconsistent and
//! invalid comparators, including the "invalid order function for sorting"
//! error and the partially sorted state it leaves behind.

// Test-only pedantic lints: short binding names mirror the Lua under test.
#![allow(clippy::many_single_char_names)]

mod common;

use common::*;
use mlua::Table;
use pob_lua::sort::{sort, sort_default};

const PER_COMPARATOR: usize = 10_500;

const COMPARATORS: [(&str, &str); 6] = [
    (
        "a.key < b.key (ties)",
        "function(a, b) return a.key < b.key end",
    ),
    ("a.key > b.key", "function(a, b) return a.key > b.key end"),
    (
        "a.key <= b.key (invalid)",
        "function(a, b) return a.key <= b.key end",
    ),
    (
        "total order (key, id)",
        "function(a, b) return a.key < b.key or (a.key == b.key and a.id < b.id) end",
    ),
    (
        "inconsistent",
        "function(a, b) return (a.key * 7 + b.key * 3) % 5 < 2 end",
    ),
    ("always true (invalid)", "function(a, b) return true end"),
];

fn rs_lt(which: usize, a: &(f64, usize), b: &(f64, usize)) -> bool {
    match which {
        0 => a.0 < b.0,
        1 => a.0 > b.0,
        2 => a.0 <= b.0,
        3 => a.0 < b.0 || (a.0 == b.0 && a.1 < b.1),
        4 => pob_lua::math::lmod(a.0 * 7.0 + b.0 * 3.0, 5.0) < 2.0,
        _ => true,
    }
}

fn gen_arrays(seed: u64, count: usize) -> Vec<Vec<f64>> {
    let mut r = Rng(seed);
    (0..count)
        .map(|_| {
            let n = if r.chance(0.05) {
                r.range(80, 300)
            } else {
                r.range(0, 60)
            } as usize;
            let kmax = *r.pick(&[1i64, 2, 3, 5, 10, 1000]);
            (0..n).map(|_| r.range(0, kmax) as f64).collect()
        })
        .collect()
}

#[test]
fn table_sort_comparators() {
    quiet_lua_errors();
    for mode in modes() {
        let lua = lua(mode);
        for (ci, (name, cmp)) in COMPARATORS.iter().enumerate() {
            let arrays = gen_arrays(900 + ci as u64, PER_COMPARATOR);
            let f = func(
                &lua,
                &format!(
                    "local sort, pcall, concat = table.sort, pcall, table.concat local cmp = {cmp} \
                     return function(A) local out = {{}} for i = 1, #A do local a = A[i] local t = {{}} \
                     for j = 1, #a do t[j] = {{ key = a[j], id = j }} end \
                     local ok, err = pcall(sort, t, cmp) local ids = {{}} for j = 1, #t do ids[j] = t[j].id end \
                     out[i] = (ok and 'ok:' or ('err:' .. tostring(err) .. ':')) .. concat(ids, ',') end return out end"
                ),
            );
            let at = lua.create_table().unwrap();
            for (i, a) in arrays.iter().enumerate() {
                at.raw_set(i + 1, seq_f64(&lua, a)).unwrap();
            }
            let out: Table = f.call(at).unwrap();
            let mut c = Check::new(format!("table.sort {name} [{mode}]"));
            let mut errors = 0;
            for (i, a) in arrays.iter().enumerate() {
                let want = String::from_utf8(bytes_of(&get(&out, i + 1)).unwrap()).unwrap();
                let mut v: Vec<(f64, usize)> =
                    a.iter().enumerate().map(|(j, &k)| (k, j + 1)).collect();
                let res = rs_catch(|| sort(&mut v, |x, y| rs_lt(ci, x, y)));
                let ids = v
                    .iter()
                    .map(|x| x.1.to_string())
                    .collect::<Vec<_>>()
                    .join(",");
                let same = match &res {
                    Ok(()) => want == format!("ok:{ids}"),
                    Err(e) => {
                        errors += 1;
                        want.starts_with("err:")
                            && want.ends_with(&format!(":{ids}"))
                            && want.contains(e.as_str())
                    }
                };
                c.ok(same, || {
                    format!("{name} keys {a:?}: lua {want} rust {res:?} {ids}")
                });
            }
            eprintln!("  ({errors} vectors raised 'invalid order function for sorting')");
            c.finish();
        }
        // Default comparator (`<`) on numbers, with NaNs and ±0.
        let mut r = Rng(950);
        let arrays: Vec<Vec<f64>> = (0..PER_COMPARATOR)
            .map(|_| {
                (0..r.range(0, 50))
                    .map(|_| match r.below(10) {
                        0 => f64::NAN,
                        1 => -0.0,
                        2 => 0.0,
                        _ => r.range(-5, 5) as f64 + if r.chance(0.3) { 0.5 } else { 0.0 },
                    })
                    .collect()
            })
            .collect();
        let f = func(
            &lua,
            &format!(
                "{ENC} local sort, pcall = table.sort, pcall return function(A) local out, oks = {{}}, {{}} local k = 0 \
                 for i = 1, #A do local t = {{}} for j = 1, #A[i] do t[j] = A[i][j] end oks[i] = pcall(sort, t) \
                 for j = 1, #A[i] do k = k + 1 E(out, k, t[j]) end end return out, oks end"
            ),
        );
        let at = lua.create_table().unwrap();
        for (i, a) in arrays.iter().enumerate() {
            at.raw_set(i + 1, seq_f64(&lua, a)).unwrap();
        }
        let (out, oks): (Table, Table) = f.call(at).unwrap();
        let mut c = Check::new(format!("table.sort default < with NaN [{mode}]"));
        let mut k = 0;
        for (i, a) in arrays.iter().enumerate() {
            let mut v = a.clone();
            let res = rs_catch(|| sort_default(&mut v));
            let lua_ok = matches!(get(&oks, i + 1), mlua::Value::Boolean(true));
            let mut same = res.is_ok() == lua_ok;
            for x in &v {
                k += 1;
                same &= same_num(num_at(&out, k), *x);
            }
            c.ok(same, || {
                format!("sort({a:?}): rust {v:?} ok={}", res.is_ok())
            });
        }
        c.finish();
    }
}
