//! L0 (calc doc §9.4, unit P2 acceptance): `LjIntMap` `pairs()` order, values
//! and `#t` equal LuaJIT's for ≥ 10k random insert/delete sequences, plus
//! `copyTable` order, `table.new` presizing and `wipeTable`.

mod common;

use common::*;
use mlua::Table;
use pob_lua::format::tostring_num;
use pob_lua::LjIntMap;

const SEQUENCES: usize = 10_000;

struct Seq {
    narr: i64, // -1 = `{}`
    nrec: i64,
    ops: Vec<(u8, i64, f64)>, // 1 = set, 2 = set nil, 3 = wipeTable
}

fn gen_key(r: &mut Rng, profile: u64, live: &[i64]) -> i64 {
    if !live.is_empty() && r.chance(0.15) {
        return *r.pick(live);
    }
    match profile {
        0 => r.range(0, 40),
        1 => r.range(1, 300),
        2 => r.range(1, 65535),
        3 => match r.below(6) {
            0 => r.range(-50, 0),
            1 => r.range(0, 20),
            2 => r.range(2_147_483_000, 2_147_484_000),
            3 => r.range(-3_000_000_000, 3_000_000_000),
            _ => r.range(1, 70000),
        },
        _ => 10000 + r.range(0, 60) * *r.pick(&[1, 2, 16, 64]),
    }
}

fn gen_seq(r: &mut Rng) -> Seq {
    let profile = r.below(5);
    let (narr, nrec) = if r.chance(0.1) {
        (r.range(0, 40), r.range(0, 40))
    } else {
        (-1, 0)
    };
    let nops = if r.chance(0.05) {
        r.range(200, 3000)
    } else {
        r.range(1, 120)
    } as usize;
    let mut live: Vec<i64> = Vec::new();
    let mut ops = Vec::with_capacity(nops);
    for i in 0..nops {
        let roll = r.below(1000);
        if roll < 5 {
            ops.push((3, 0, 0.0));
            live.clear();
        } else if roll < 220 && !live.is_empty() {
            let idx = r.below(live.len() as u64) as usize;
            let k = live.swap_remove(idx);
            ops.push((2, k, 0.0));
        } else if roll < 290 {
            ops.push((2, gen_key(r, profile, &live), 0.0)); // often a missing key: inserts a dead node
        } else {
            let k = if profile == 1 && r.chance(0.5) {
                i as i64 + 1
            } else {
                gen_key(r, profile, &live)
            };
            ops.push((1, k, i as f64 + 1.0));
            if !live.contains(&k) {
                live.push(k);
            }
        }
    }
    Seq { narr, nrec, ops }
}

fn rust_result(s: &Seq) -> String {
    let mut t: LjIntMap<f64> = if s.narr >= 0 {
        LjIntMap::with_sizes(s.narr as u32, s.nrec as u32)
    } else {
        LjIntMap::new()
    };
    for &(op, k, v) in &s.ops {
        match op {
            1 => t.set(k, Some(v)),
            2 => t.set(k, None),
            _ => t.wipe(),
        }
    }
    let keys: Vec<String> = t
        .pairs()
        .iter()
        .map(|(k, v)| format!("{}={}", tostring_num(*k as f64), tostring_num(*v)))
        .collect();
    let copy = t.copyTable();
    let ck: Vec<String> = copy
        .keys()
        .iter()
        .map(|k| tostring_num(*k as f64))
        .collect();
    format!("{}|{}|{}", keys.join(","), t.len(), ck.join(","))
}

#[test]
fn ljintmap_order_matches_luajit() {
    let mut r = Rng(1234);
    let seqs: Vec<Seq> = (0..SEQUENCES).map(|_| gen_seq(&mut r)).collect();
    let total_ops: usize = seqs.iter().map(|s| s.ops.len()).sum();
    eprintln!("  {SEQUENCES} sequences, {total_ops} operations");
    for mode in modes() {
        let lua = lua(mode);
        let f = func(
            &lua,
            "local tnew = require('table.new') local concat, pairs = table.concat, pairs \
             return function(S) local out = {} for i = 1, #S do local q = S[i] \
               local t = (q[1] >= 0) and tnew(q[1], q[2]) or {} local j = 3 \
               while j <= #q do local op, k = q[j], q[j+1] \
                 if op == 1 then t[k] = q[j+2] elseif op == 2 then t[k] = nil else for kk in pairs(t) do t[kk] = nil end end \
                 j = j + 3 end \
               local keys = {} for k, v in pairs(t) do keys[#keys+1] = k .. '=' .. v end \
               local c = {} for k, v in pairs(t) do c[k] = v end \
               local ck = {} for k in pairs(c) do ck[#ck+1] = k end \
               out[i] = concat(keys, ',') .. '|' .. #t .. '|' .. concat(ck, ',') end return out end",
        );
        let st = lua.create_table().unwrap();
        for (i, s) in seqs.iter().enumerate() {
            let mut flat = vec![s.narr as f64, s.nrec as f64];
            for &(op, k, v) in &s.ops {
                flat.extend_from_slice(&[f64::from(op), k as f64, v]);
            }
            st.raw_set(i + 1, seq_f64(&lua, &flat)).unwrap();
        }
        let out: Table = f.call(st).unwrap();
        let mut c = Check::new(format!("LjIntMap pairs/len/copyTable [{mode}]"));
        for (i, s) in seqs.iter().enumerate() {
            let want = String::from_utf8(bytes_of(&get(&out, i + 1)).unwrap()).unwrap();
            let got = rust_result(s);
            c.ok(want == got, || {
                let short = |x: &str| x.chars().take(300).collect::<String>();
                format!(
                    "seq {i} (narr {}, nrec {}, {} ops): lua {} rust {}",
                    s.narr,
                    s.nrec,
                    s.ops.len(),
                    short(&want),
                    short(&got)
                )
            });
        }
        c.finish();
    }
}

#[test]
fn doc_example_matches_luajit() {
    let keys = [52031, 1031, 26725, 55485, 33545, 6, 7, 1, 2, 3, 99999, 4184];
    let lua = lua("jit.off");
    let order: String = lua
        .load(format!(
            "local t = {{}} for _, k in ipairs({{ {} }}) do t[k] = true end local o = {{}} for k in pairs(t) do o[#o+1] = k end return table.concat(o, ',')",
            keys.iter().map(ToString::to_string).collect::<Vec<_>>().join(",")
        ))
        .eval()
        .unwrap();
    let mut t = LjIntMap::new();
    for k in keys {
        t.set(k, Some(true));
    }
    let got = t
        .keys()
        .iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>()
        .join(",");
    eprintln!("  doc example: LuaJIT {order}, Rust {got}");
    assert_eq!(got, order);
}
