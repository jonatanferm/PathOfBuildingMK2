//! L0 (calc doc §9.4): `for_ipairs!` / `ipairs_live` against LuaJIT `ipairs`
//! when the loop body appends to the list and punches holes into it.

mod common;

use common::*;
use pob_lua::field::Shared;
use pob_lua::ipairs::ipairs_live;
use pob_oracle::mlua::Table;

#[test]
fn ipairs_live_semantics() {
    let mut r = Rng(1111);
    let lists: Vec<Vec<f64>> = (0..10_000)
        .map(|_| (0..r.range(0, 30)).map(|_| r.range(1, 60) as f64).collect())
        .collect();
    for mode in MODES {
        let lua = lua(mode);
        let f = func(
            &lua,
            "local ipairs, concat = ipairs, table.concat \
             return function(L, holes) local out = {} for i = 1, #L do local src = L[i] local t, n = {}, #src \
               for j = 1, n do t[j] = src[j] end local seen = {} \
               for k, v in ipairs(t) do seen[#seen+1] = k .. ':' .. v \
                 if v % 3 == 0 and #seen < 200 then n = n + 1 t[n] = v + 1 end \
                 if holes and v % 7 == 0 then t[k + 2] = nil end end \
               out[i] = concat(seen, ',') end return out end",
        );
        let lt = lua.create_table().unwrap();
        for (i, l) in lists.iter().enumerate() {
            lt.raw_set(i + 1, seq_f64(&lua, l)).unwrap();
        }
        let out: Table = f.call((lt.clone(), true)).unwrap();
        let out2: Table = f.call((lt, false)).unwrap();
        let mut c1 = Check::new(format!("ipairs_live (holes + appends) [{mode}]"));
        let mut c2 = Check::new(format!("for_ipairs! on Shared<Vec> (appends) [{mode}]"));
        for (i, l) in lists.iter().enumerate() {
            let want = String::from_utf8(bytes_of(&get(&out, i + 1)).unwrap()).unwrap();
            // ipairs_live: holes are None.
            let mut v: Vec<Option<f64>> = l.iter().copied().map(Some).collect();
            let mut seen: Vec<String> = Vec::new();
            ipairs_live(&mut v, |k, x, list| {
                seen.push(format!("{k}:{x}"));
                if x % 3.0 == 0.0 && seen.len() < 200 {
                    list.push(Some(x + 1.0));
                }
                if x % 7.0 == 0.0 && k + 2 <= list.len() {
                    list[k + 1] = None;
                }
            });
            let got = seen.join(",");
            c1.ok(want == got, || format!("{l:?}: lua {want} rust {got}"));
            // for_ipairs! (no holes: Shared<Vec<T>> is hole-free).
            let want = String::from_utf8(bytes_of(&get(&out2, i + 1)).unwrap()).unwrap();
            {
                let list = Shared::new(l.clone());
                let mut seen2: Vec<String> = Vec::new();
                pob_lua::for_ipairs!((k, x) in list => {
                    seen2.push(format!("{k}:{x}"));
                    if x % 3.0 == 0.0 && seen2.len() < 200 {
                        list.push(x + 1.0);
                    }
                });
                let got2 = seen2.join(",");
                c2.ok(want == got2, || format!("{l:?}: lua {want} rust {got2}"));
            }
        }
        c1.finish();
        c2.finish();
    }
}
