//! HYG1: a table constructor's `pairs()` order is field order (calc doc §3.4).
//!
//! Moved here from `tests/hyg1_len.rs` by GOLD1 (docs/standalone/GOLDEN-PLAN.md §1.6). It is
//! Rust against Rust (no LuaJIT), and it guards the product's table iteration order, which the
//! order-dependent goldens rely on; `hyg1_len.rs` goes with `mlua` at the cut-over. The
//! constructor generator below is hyg1_len's, copied unchanged, so the same seed draws the same
//! 10 000 constructors.

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
const FLOATS: [f64; 9] = [
    0.5,
    1.5,
    2.25,
    929.1,
    -0.5,
    2.0,
    -0.0,
    1e300,
    9_007_199_254_740_992.0,
];

impl K {
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

fn num(v: f64) -> String {
    pob_lua::tostring_num(v)
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
