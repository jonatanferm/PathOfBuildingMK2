//! HYG1: `Table::len` (`#t`) equals LuaJIT's border (`jit.off()`, the oracle's
//! interpreter) for random construction + mutation sequences.
//!
//! Each sequence builds a table (`{}` or a constructor mixing constant and
//! run-time items, `name = v` and `[k] = v` fields), then applies stores of
//! values and of nil, `table.insert` (both forms), `table.remove` (both forms),
//! `wipeTable` and `copyTable`, recording `#t` after every step; at the end the
//! contents are compared too. The same sequence runs as generated Lua source
//! in LuaJIT and through `Table`/`tbl!`'s API (`Table::from_ctor`).
//!
//! * `table_len_matches_luajit`: 10 000 sequences over number (integer and,
//!   since FUP1, float) and boolean
//!   keys, which LuaJIT lays out deterministically: 0 mismatches required.
//! * `table_len_with_string_keys`: string keys too. LuaJIT hashes strings by a
//!   per-process id (`lj_str.c` `sid`, reseeded from a PRNG), so there the
//!   border of a holed table can differ between LuaJIT processes. The test
//!   requires every Rust `len()` to be a valid border, and reports how often
//!   Rust and LuaJIT disagree next to how often two LuaJIT states disagree.

// Test-only pedantic lints: short binding names mirror the Lua under test.
#![allow(clippy::many_single_char_names)]

use std::fmt::Write as _;

use mlua::{Function, Lua};
use pob_calc::lua_table::{Key, Table, TableField, Val};
use pob_lua::Name;

const SEQUENCES: usize = 10_000;

/// SplitMix64.
struct Rng(u64);

impl Rng {
    fn next_u64(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9e37_79b9_7f4a_7c15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
        z ^ (z >> 31)
    }
    fn below(&mut self, n: u64) -> u64 {
        self.next_u64() % n
    }
    fn range(&mut self, lo: i64, hi: i64) -> i64 {
        lo + self.below((hi - lo + 1) as u64) as i64
    }
    fn chance(&mut self, p: f64) -> bool {
        (self.next_u64() >> 11) as f64 / (1u64 << 53) as f64 <= p
    }
}

#[derive(Clone, Copy, Debug)]
enum K {
    I(i64),
    B(bool),
    S(&'static str),
    /// FUP1: a float key, written as a float literal (integral ones and `-0.0`
    /// normalise to the integer key, the others are hash keys).
    F(f64),
}

const NAMES: [&str; 6] = ["a", "b", "c", "Life", "x1", "k"];

/// FUP1: float keys (`929.1` is the statOrder of the crafted flask in IT-B).
const FLOATS: [f64; 9] = [0.5, 1.5, 2.25, 929.1, -0.5, 2.0, -0.0, 1e300, 9_007_199_254_740_992.0];

impl K {
    fn lua(self) -> String {
        match self {
            K::I(i) => i.to_string(),
            K::B(b) => b.to_string(),
            K::S(s) => format!("{s:?}"),
            K::F(f) => format!("{f:?}"),
        }
    }
    fn key(self) -> Key {
        match self {
            K::I(i) => Key::I(i),
            K::B(b) => Key::B(b),
            K::S(s) => Key::S(Name::from(s)),
            K::F(f) => Key::from(f),
        }
    }
}

#[derive(Clone, Debug)]
enum FieldKind {
    Item,
    Named(&'static str),
    Bracket { key: K, konst: bool },
}

#[derive(Clone, Debug)]
struct Field {
    kind: FieldKind,
    konst: bool,
    /// None = nil.
    val: Option<f64>,
}

#[derive(Clone, Debug)]
enum Op {
    Set(K, Option<f64>),
    Push(f64),
    Insert(i64, f64),
    Remove(Option<i64>),
    Wipe,
    Copy,
}

#[derive(Debug)]
struct Seq {
    /// None = `{}`.
    ctor: Option<Vec<Field>>,
    ops: Vec<Op>,
}

fn gen_int_key(r: &mut Rng, profile: u64) -> i64 {
    match profile {
        0 => r.range(0, 12),
        1 => r.range(1, 40),
        2 => r.range(-3, 300),
        3 => match r.below(5) {
            0 => r.range(-50, 0),
            1 => r.range(2_147_483_640, 2_147_483_660),
            2 => r.range(1, 70_000),
            _ => r.range(0, 20),
        },
        _ => r.range(1, 8),
    }
}

fn gen_key(r: &mut Rng, profile: u64, strings: bool) -> K {
    let roll = r.below(100);
    if roll < 4 {
        K::B(r.chance(0.5))
    } else if roll < 9 {
        K::F(FLOATS[r.below(FLOATS.len() as u64) as usize])
    } else if strings && roll < 30 {
        K::S(NAMES[r.below(NAMES.len() as u64) as usize])
    } else {
        K::I(gen_int_key(r, profile))
    }
}

fn gen_val(r: &mut Rng, i: usize) -> Option<f64> {
    (!r.chance(0.08)).then_some(i as f64 + 1.0)
}

fn gen_ctor(r: &mut Rng, profile: u64, strings: bool) -> Option<Vec<Field>> {
    if r.chance(0.3) {
        return None;
    }
    let n = match r.below(1000) {
        0..=1 => r.range(2040, 2060) as usize, // narr around 0x7ff (BC_TNEW 0x801)
        2..=30 => r.range(250, 300) as usize,  // TSETV above key 255
        _ => r.range(0, 14) as usize,
    };
    // Mostly-constant / mostly-variable / mixed constructors.
    let pconst = [0.0, 1.0, 0.5, 0.9][r.below(4) as usize];
    let mut fields = Vec::with_capacity(n);
    for i in 0..n {
        let konst = r.chance(pconst);
        let val = gen_val(r, i);
        let kind = match r.below(100) {
            0..=69 => FieldKind::Item,
            70..=79 if strings => FieldKind::Named(NAMES[r.below(NAMES.len() as u64) as usize]),
            _ => {
                let key = if r.chance(0.15) {
                    K::I(0)
                } else {
                    gen_key(r, profile, strings)
                };
                FieldKind::Bracket {
                    key,
                    konst: r.chance(0.7),
                }
            }
        };
        fields.push(Field { kind, konst, val });
    }
    Some(fields)
}

fn gen_seq(r: &mut Rng, strings: bool) -> Seq {
    let profile = r.below(5);
    let ctor = gen_ctor(r, profile, strings);
    let nops = if r.chance(0.05) {
        r.range(100, 400)
    } else {
        r.range(1, 40)
    } as usize;
    let mut ops = Vec::with_capacity(nops);
    for i in 0..nops {
        let op = match r.below(1000) {
            0..=399 => Op::Set(gen_key(r, profile, strings), Some(i as f64 + 100.0)),
            400..=649 => Op::Set(gen_key(r, profile, strings), None),
            650..=749 => Op::Push(i as f64 + 100.0),
            750..=829 => Op::Insert(r.range(-2, 14), i as f64 + 100.0),
            830..=899 => Op::Remove(None),
            900..=969 => Op::Remove(Some(r.range(-1, 14))),
            970..=984 => Op::Wipe,
            _ => Op::Copy,
        };
        ops.push(op);
    }
    Seq { ctor, ops }
}

fn num(v: f64) -> String {
    pob_lua::tostring_num(v)
}

/// The sequence as a Lua chunk returning `lens|contents`.
fn lua_source(s: &Seq) -> String {
    let mut src = String::new();
    // A holds the run-time values (A[0] is nil): `A[i]` is not a constant.
    let mut avals: Vec<String> = Vec::new();
    let mut runtime = |v: String| {
        avals.push(v);
        format!("A[{}]", avals.len())
    };
    let mut ctor = String::from("{}");
    if let Some(fields) = &s.ctor {
        let mut parts = Vec::with_capacity(fields.len());
        for f in fields {
            let val = match (f.konst, f.val) {
                (true, Some(v)) => num(v),
                (true, None) => "nil".to_owned(),
                (false, Some(v)) => runtime(num(v)),
                (false, None) => "A[0]".to_owned(),
            };
            parts.push(match &f.kind {
                FieldKind::Item => val,
                FieldKind::Named(n) => format!("{n} = {val}"),
                FieldKind::Bracket { key, konst: true } => format!("[{}] = {val}", key.lua()),
                FieldKind::Bracket { key, konst: false } => {
                    format!("[{}] = {val}", runtime(key.lua()))
                }
            });
        }
        ctor = format!("{{{}}}", parts.join(", "));
    }
    let _ = write!(src, "local A = {{");
    for (i, v) in avals.iter().enumerate() {
        let _ = write!(src, "[{}] = {v}, ", i + 1);
    }
    let _ = writeln!(src, "}}");
    let _ = writeln!(src, "local t = {ctor}");
    let _ = writeln!(src, "local L = {{ #t }}");
    for op in &s.ops {
        let line = match op {
            Op::Set(k, Some(v)) => format!("t[{}] = {}", k.lua(), num(*v)),
            Op::Set(k, None) => format!("t[{}] = nil", k.lua()),
            Op::Push(v) => format!("table.insert(t, {})", num(*v)),
            Op::Insert(p, v) => format!("table.insert(t, {p}, {})", num(*v)),
            Op::Remove(None) => "table.remove(t)".to_owned(),
            Op::Remove(Some(p)) => format!("table.remove(t, {p})"),
            Op::Wipe => "for k in pairs(t) do t[k] = nil end".to_owned(),
            Op::Copy => "do local c = {} for k, v in pairs(t) do c[k] = v end t = c end".to_owned(),
        };
        let _ = writeln!(src, "{line}; L[#L + 1] = #t");
    }
    src.push_str(
        "local C = {} for k, v in pairs(t) do C[#C + 1] = type(k):sub(1, 1) .. tostring(k) .. '=' .. tostring(v) end \
         table.sort(C) return table.concat(L, ',') .. '|' .. table.concat(C, ',')\n",
    );
    src
}

fn key_text(k: Key) -> String {
    match k {
        Key::I(i) => format!("n{}", num(i as f64)),
        Key::B(b) => format!("b{b}"),
        Key::S(s) => format!("s{s}"),
        Key::F(f) => format!("n{}", num(f.get())),
    }
}

fn val_text(v: &Val) -> String {
    match v {
        Val::Num(n) => num(*n),
        other => format!("{other:?}"),
    }
}

/// A valid border: `t[n] ~= nil` (or n = 0) and `t[n + 1] == nil`.
fn is_border(t: &Table, n: usize) -> bool {
    (n == 0 || !t.rawget(Key::I(n as i64)).is_nil()) && t.rawget(Key::I(n as i64 + 1)).is_nil()
}

/// Runs the sequence on `Table`; returns `lens|contents` and whether every
/// `len()` was a valid border.
fn rust_ctor(fields: &[Field]) -> Table {
    Table::from_ctor(
        fields
            .iter()
            .map(|f| {
                let val = f.val.map_or(Val::Nil, Val::Num);
                match &f.kind {
                    FieldKind::Item => TableField::item(f.konst, val),
                    FieldKind::Named(n) => TableField::named(Name::from(n), f.konst, val),
                    FieldKind::Bracket { key, konst } => {
                        // `[-0.0]` is no constant key: LuaJIT does not fold the
                        // negation of a zero (`lj_parse.c` `bcemit_unop`,
                        // `!expr_numiszero(e)`), so the key is stored at run time.
                        let konst = *konst && !matches!(key, K::F(f) if *f == 0.0);
                        TableField::bracket(key.key(), konst, f.konst, val)
                    }
                }
            })
            .collect(),
    )
}

fn rust_result(s: &Seq) -> (String, bool) {
    let mut t = match &s.ctor {
        None => Table::new(),
        Some(fields) => rust_ctor(fields),
    };
    let mut borders_ok = is_border(&t, t.len());
    let mut lens = vec![t.len().to_string()];
    for op in &s.ops {
        match op {
            Op::Set(k, v) => t.set(k.key(), v.map_or(Val::Nil, Val::Num)),
            Op::Push(v) => t.push(*v),
            Op::Insert(p, v) => t.insert(*p, *v),
            Op::Remove(p) => {
                t.remove(*p);
            }
            Op::Wipe => t.wipe(),
            Op::Copy => t = t.copyTable(false),
        }
        let n = t.len();
        borders_ok &= is_border(&t, n);
        lens.push(n.to_string());
    }
    let mut c: Vec<String> = t
        .pairs()
        .map(|(k, v)| format!("{}={}", key_text(k), val_text(&v)))
        .collect();
    c.sort();
    (format!("{}|{}", lens.join(","), c.join(",")), borders_ok)
}

fn oracle_state(junk_strings: usize) -> (Lua, Function) {
    let lua = Lua::new();
    lua.load("jit.off() jit.flush()").exec().expect("jit.off");
    // Shift the string ids of this state (lj_str.c sid) by interning strings.
    lua.load(format!(
        "JUNK = {{}} for i = 1, {junk_strings} do JUNK[i] = 'junk' .. i end"
    ))
    .exec()
    .expect("junk strings");
    let f = lua
        .load("local load = load return function(src) return assert(load(src))() end")
        .eval::<Function>()
        .expect("runner");
    (lua, f)
}

fn run_lua(f: &Function, s: &Seq) -> String {
    let src = lua_source(s);
    f.call::<String>(src.as_str())
        .unwrap_or_else(|e| panic!("lua: {e}\n{src}"))
}

fn short(x: &str) -> String {
    x.chars().take(400).collect()
}

#[test]
fn table_len_matches_luajit() {
    let mut r = Rng(0x4859_4731); // "HYG1"
    let seqs: Vec<Seq> = (0..SEQUENCES).map(|_| gen_seq(&mut r, false)).collect();
    let ops: usize = seqs.iter().map(|s| s.ops.len()).sum();
    let ctors = seqs.iter().filter(|s| s.ctor.is_some()).count();
    let (_lua, f) = oracle_state(0);
    let mut bad = Vec::new();
    for (i, s) in seqs.iter().enumerate() {
        let want = run_lua(&f, s);
        let (got, borders_ok) = rust_result(s);
        assert!(borders_ok, "seq {i}: len() is not a border");
        if want != got {
            bad.push(format!(
                "seq {i}: {s:?}\n  lua  {}\n  rust {}",
                short(&want),
                short(&got)
            ));
        }
    }
    eprintln!(
        "HYG1 #t vs LuaJIT [jit.off], number/boolean keys: {SEQUENCES} sequences ({ctors} constructors), \
         {ops} operations, {} mismatches",
        bad.len()
    );
    for b in bad.iter().take(5) {
        eprintln!("{b}");
    }
    assert!(bad.is_empty(), "{} mismatching sequences", bad.len());
}

/// Storage order (calc doc §3.4: `pairs` follows insertion) of a constructor
/// is field order, whatever its constant / run-time split: `from_ctor`'s
/// `pairs()` equals that of a plain table that stores every field in turn
/// (items are keys 1, 2, … in turn). That reference takes the LAST field of a
/// duplicated key, where Lua takes the last run-time one (the template is in
/// place first), so constructors whose contents differ for that reason are
/// not compared.
#[test]
fn ctor_pairs_follow_field_order() {
    let mut r = Rng(0x4859_4733);
    let (mut checked, mut dup) = (0, 0);
    for _ in 0..SEQUENCES {
        let profile = r.below(5);
        let Some(fields) = gen_ctor(&mut r, profile, true) else {
            continue;
        };
        let t = rust_ctor(&fields);
        let want = Table::new();
        let mut item = 0;
        for f in &fields {
            let k = match &f.kind {
                FieldKind::Item => {
                    item += 1;
                    Key::I(item)
                }
                FieldKind::Named(n) => Key::S(Name::from(n)),
                FieldKind::Bracket { key, .. } => key.key(),
            };
            want.set(k, f.val.map_or(Val::Nil, Val::Num));
        }
        let pairs = |t: &Table| {
            t.pairs()
                .map(|(k, v)| format!("{}={}", key_text(k), val_text(&v)))
                .collect::<Vec<_>>()
        };
        let (got, want) = (pairs(&t), pairs(&want));
        let sorted = |v: &[String]| {
            let mut v = v.to_vec();
            v.sort();
            v
        };
        if sorted(&got) != sorted(&want) {
            dup += 1;
            continue;
        }
        assert_eq!(got, want, "{fields:?}");
        checked += 1;
    }
    eprintln!(
        "HYG1 constructor pairs order: {checked} constructors in field order \
         ({dup} not compared: a duplicate key whose template value Lua overrides)"
    );
    assert!(checked > 5_000);
}

#[test]
fn table_len_with_string_keys() {
    const N: usize = 3_000;
    let mut r = Rng(0x4859_4732);
    let seqs: Vec<Seq> = (0..N).map(|_| gen_seq(&mut r, true)).collect();
    let (_a, fa) = oracle_state(0);
    let (_b, fb) = oracle_state(777);
    let (mut rust_vs_lua, mut lua_vs_lua) = (0usize, 0usize);
    for (i, s) in seqs.iter().enumerate() {
        let a = run_lua(&fa, s);
        let b = run_lua(&fb, s);
        let (got, borders_ok) = rust_result(s);
        assert!(borders_ok, "seq {i}: len() is not a border: {s:?}");
        rust_vs_lua += usize::from(a != got);
        lua_vs_lua += usize::from(a != b);
    }
    eprintln!(
        "HYG1 #t with string keys [jit.off], {N} sequences: Rust vs LuaJIT {rust_vs_lua} differ; \
         LuaJIT vs LuaJIT (other string ids) {lua_vs_lua} differ; every Rust len() is a border"
    );
}
