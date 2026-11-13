//! L0 (calc doc §9.4): `pob_lua::math` and `pob_lua::bit` against LuaJIT,
//! ≥ 10k random + edge vectors per function, in JIT and interpreter mode.
//! PoB's Common.lua helpers run from the real Common.lua source.

// Test-only pedantic lints: short binding names mirror the Lua under test.
#![allow(clippy::many_single_char_names)]

mod common;

use common::*;
use pob_lua::bit;
use pob_lua::math::*;

const N: usize = 12_000;

/// Common.lua's header locals plus lines 709-800 and 1013-1023, verbatim.
fn common_lua_chunk() -> String {
    format!(
        "{}\n{}\n{}\nreturn {{ round = round, floor = floor, roundSymmetric = roundSymmetric, \
         alwaysPositiveRound = alwaysPositiveRound, floorSymmetric = floorSymmetric, \
         ceilSymmetric = ceilSymmetric, triangular = triangular, ceil_b = ceil_b, floor_b = floor_b }}",
        pob_lines("Modules/Common.lua", 7, 21),
        pob_lines("Modules/Common.lua", 709, 800),
        pob_lines("Modules/Common.lua", 1013, 1023),
    )
}

fn unary(name: &str, expr: &str, rs: impl Fn(f64) -> f64, seed: u64) {
    for mode in modes() {
        let lua = lua(mode);
        let xs = numbers(seed, N);
        let f = func(&lua, &format!("{ENC} return function(xs) local out = {{}} for i = 1, #xs do local x = xs[i] E(out, i, {expr}) end return out end"));
        let out: mlua::Table = f.call(seq_f64(&lua, &xs)).unwrap();
        let mut c = Check::new(format!("{name} [{mode}]"));
        for (i, &x) in xs.iter().enumerate() {
            let want = num_at(&out, i + 1);
            let got = rs(x);
            c.num(mode, want, got, || {
                format!(
                    "{name}({x:e} / {:#x}): lua {want:e} ({:#x}) rust {got:e} ({:#x})",
                    x.to_bits(),
                    want.to_bits(),
                    got.to_bits()
                )
            });
        }
        c.finish();
    }
}

fn binary(
    name: &str,
    expr: &str,
    rs: impl Fn(f64, f64) -> f64,
    seed: u64,
    ys_gen: impl Fn(&mut Rng) -> f64,
) {
    binary_alt(name, expr, rs, seed, ys_gen, None);
}

/// `jit_alt`: what a compiled trace computes instead, where LuaJIT's JIT
/// folds the expression differently from the interpreter (e.g. `x ^ 2` →
/// `x * x`). In `jit.on` mode a result equal to `jit_alt` is tolerated and
/// counted; the `jit.off` run is always bit-exact.
fn binary_alt(
    name: &str,
    expr: &str,
    rs: impl Fn(f64, f64) -> f64,
    seed: u64,
    ys_gen: impl Fn(&mut Rng) -> f64,
    jit_alt: Option<fn(f64, f64) -> f64>,
) {
    for mode in modes() {
        let lua = lua(mode);
        let xs = numbers(seed, N);
        let mut r = Rng(seed ^ 0x5555);
        let sp = special_numbers();
        let mut ys: Vec<f64> = Vec::with_capacity(xs.len());
        for i in 0..xs.len() {
            ys.push(canon(if i < sp.len() * 2 {
                sp[i % sp.len()]
            } else {
                ys_gen(&mut r)
            }));
        }
        // Cross product of the specials with themselves as extra vectors.
        let mut xs2 = xs.clone();
        for &a in &sp {
            for &b in sp.iter().step_by(7) {
                xs2.push(canon(a));
                ys.push(canon(b));
            }
        }
        let f = func(&lua, &format!("{ENC} return function(xs, ys) local out = {{}} for i = 1, #xs do local x, y = xs[i], ys[i] E(out, i, {expr}) end return out end"));
        let out: mlua::Table = f.call((seq_f64(&lua, &xs2), seq_f64(&lua, &ys))).unwrap();
        let mut c = Check::new(format!("{name} [{mode}]"));
        for i in 0..xs2.len() {
            let (x, y) = (xs2[i], ys[i]);
            let want = num_at(&out, i + 1);
            let got = rs(x, y);
            if let Some(alt) = jit_alt {
                if mode == "jit.on" && !same_num(want, got) && same_num(want, alt(x, y)) {
                    c.total += 1;
                    c.jit_zero_sign += 1;
                    eprintln!("  JIT-only fold: {name}({x:e}, {y:e}) trace {want:e} interpreter/rust {got:e}");
                    continue;
                }
            }
            c.num(mode, want, got, || {
                format!(
                    "{name}({x:e}, {y:e}): lua {want:e} ({:#x}) rust {got:e} ({:#x})",
                    want.to_bits(),
                    got.to_bits()
                )
            });
        }
        c.finish();
    }
}

#[test]
fn lj_math_unary() {
    unary("math.floor", "math.floor(x)", m_floor, 1);
    unary("math.ceil", "math.ceil(x)", m_ceil, 2);
    unary("math.abs", "math.abs(x)", m_abs, 3);
    unary("math.sqrt", "math.sqrt(x)", m_sqrt, 4);
    unary("math.exp", "math.exp(x)", m_exp, 5);
    unary("math.log", "math.log(x)", |x| m_log(x, None), 6);
    unary("math.log10", "math.log10(x)", m_log10, 7);
    unary("math.modf[1]", "(math.modf(x))", |x| m_modf(x).0, 8);
    unary(
        "math.modf[2]",
        "select(2, math.modf(x))",
        |x| m_modf(x).1,
        9,
    );
    unary(
        "math.min(x, x+1, -x)",
        "math.min(x, x + 1, -x)",
        |x| m_min_n(&[x, x + 1.0, -x]),
        10,
    );
    unary(
        "math.max(x, x*0.5, 7)",
        "math.max(x, x * 0.5, 7)",
        |x| m_max_n(&[x, x * 0.5, 7.0]),
        11,
    );
}

#[test]
fn lj_math_binary() {
    let gen = |r: &mut Rng| random_number(r);
    binary("math.min", "math.min(x, y)", m_min, 21, gen);
    binary("math.max", "math.max(x, y)", m_max, 22, gen);
    binary("x % y", "x % y", lmod, 23, gen);
    binary("x ^ y", "x ^ y", lpow, 24, |r| {
        if r.chance(0.5) {
            r.range(-10, 10) as f64
        } else {
            random_number(r)
        }
    });
    binary("math.pow", "math.pow(x, y)", m_pow, 25, |r| {
        r.range(-4, 6) as f64 * 0.5
    });
    binary(
        "math.log(x, b)",
        "math.log(x, y)",
        |x, y| m_log(x, Some(y)),
        26,
        |r| *r.pick(&[2.0, 10.0, 3.0, 0.5, 1.5, std::f64::consts::E]),
    );
    binary_alt(
        "x ^ 2 (JIT folds to x*x)",
        "x ^ 2 + y * 0",
        |x, y| lpow(x, 2.0) + y * 0.0,
        27,
        gen,
        Some(|x, y| x * x + y * 0.0),
    );
}

#[test]
fn common_lua_rounding() {
    if pob_src().is_none() {
        return; // POB_ALLOW_SKIP=1
    }
    for mode in modes() {
        let lua = lua(mode);
        let fns: mlua::Table = lua
            .load(common_lua_chunk())
            .eval()
            .expect("Common.lua chunk");
        let xs = numbers(31, N);
        let mut r = Rng(77);
        let decs: Vec<Option<i32>> = (0..xs.len())
            .map(|_| {
                if r.chance(0.3) {
                    None
                } else {
                    Some(r.range(-2, 6) as i32)
                }
            })
            .collect();
        let decs_lua = lua.create_table().unwrap();
        for (i, d) in decs.iter().enumerate() {
            if let Some(d) = d {
                decs_lua.raw_set(i + 1, *d).unwrap();
            }
        }
        let runner = func(
            &lua,
            &format!("{ENC} return function(f, xs, ds, n) local out = {{}} for i = 1, n do E(out, i, f(xs[i], ds[i])) end return out end"),
        );
        type F = fn(f64, Option<i32>) -> f64;
        let cases: [(&str, F); 6] = [
            ("round", round),
            ("floor", floor),
            ("roundSymmetric", roundSymmetric),
            ("alwaysPositiveRound", alwaysPositiveRound),
            ("floorSymmetric", floorSymmetric),
            ("ceilSymmetric", ceilSymmetric),
        ];
        for (name, rs) in cases {
            let f: mlua::Function = fns.get(name).unwrap();
            let out: mlua::Table = runner
                .call((f, seq_f64(&lua, &xs), decs_lua.clone(), xs.len()))
                .unwrap();
            let mut c = Check::new(format!("Common.lua {name} [{mode}]"));
            for i in 0..xs.len() {
                let want = num_at(&out, i + 1);
                let got = rs(xs[i], decs[i]);
                c.num(mode, want, got, || {
                    format!(
                        "{name}({:e}, {:?}): lua {want:e} rust {got:e}",
                        xs[i], decs[i]
                    )
                });
            }
            c.finish();
        }
        // triangular, ceil_b, floor_b
        let bases: Vec<f64> = (0..xs.len())
            .map(|i| [1.0, 5.0, 0.5, 3.0, 0.1, 100.0][i % 6])
            .collect();
        for (name, rs) in [("triangular", 0usize), ("ceil_b", 1), ("floor_b", 2)] {
            let f: mlua::Function = fns.get(name).unwrap();
            let out: mlua::Table = runner
                .call((f, seq_f64(&lua, &xs), seq_f64(&lua, &bases), xs.len()))
                .unwrap();
            let mut c = Check::new(format!("Common.lua {name} [{mode}]"));
            for i in 0..xs.len() {
                let want = num_at(&out, i + 1);
                let got = match rs {
                    0 => triangular(xs[i]),
                    1 => ceil_b(xs[i], bases[i]),
                    _ => floor_b(xs[i], bases[i]),
                };
                c.num(mode, want, got, || {
                    format!(
                        "{name}({:e}, {}): lua {want:e} rust {got:e}",
                        xs[i], bases[i]
                    )
                });
            }
            c.finish();
        }
    }
}

#[test]
fn lj_bit() {
    for mode in modes() {
        let lua = lua(mode);
        let mut xs = numbers(41, N);
        let mut r = Rng(42);
        for x in xs.iter_mut().skip(200) {
            if r.chance(0.5) {
                *x = canon(
                    r.range(-(1 << 40), 1 << 40) as f64 + if r.chance(0.2) { 0.5 } else { 0.0 },
                );
            }
        }
        // NaN/inf conversions are implementation-defined bit patterns; keep them.
        let ys: Vec<f64> = (0..xs.len())
            .map(|_| {
                canon(if r.chance(0.5) {
                    r.range(-40, 40) as f64
                } else {
                    random_number(&mut r)
                })
            })
            .collect();
        let f = func(
            &lua,
            &format!("{ENC} return function(xs, ys) local o = {{}} local b = bit for i = 1, #xs do local x, y = xs[i], ys[i] \
             local k = (i - 1) * 11 \
             E(o, k+1, b.tobit(x)) E(o, k+2, b.band(x, y)) E(o, k+3, b.bor(x, y)) E(o, k+4, b.bxor(x, y)) \
             E(o, k+5, b.bnot(x)) E(o, k+6, b.lshift(x, y)) E(o, k+7, b.rshift(x, y)) E(o, k+8, b.arshift(x, y)) \
             E(o, k+9, b.rol(x, y)) E(o, k+10, b.ror(x, y)) E(o, k+11, b.bswap(x)) end return o end"),
        );
        let out: mlua::Table = f.call((seq_f64(&lua, &xs), seq_f64(&lua, &ys))).unwrap();
        let names = [
            "tobit", "band", "bor", "bxor", "bnot", "lshift", "rshift", "arshift", "rol", "ror",
            "bswap",
        ];
        let mut checks: Vec<Check> = names
            .iter()
            .map(|n| Check::new(format!("bit.{n} [{mode}]")))
            .collect();
        for i in 0..xs.len() {
            let (x, y) = (xs[i], ys[i]);
            let (a, b) = (bit::tou32(x), bit::tou32(y));
            let got = [
                f64::from(bit::tobit(x)),
                bit::tonum(bit::band(a, b)),
                bit::tonum(bit::bor(a, b)),
                bit::tonum(bit::bxor(a, b)),
                bit::tonum(bit::bnot(a)),
                bit::tonum(bit::lshift(a, b)),
                bit::tonum(bit::rshift(a, b)),
                bit::tonum(bit::arshift(a, b)),
                bit::tonum(bit::rol(a, b)),
                bit::tonum(bit::ror(a, b)),
                bit::tonum(bit::bswap(a)),
            ];
            for (j, c) in checks.iter_mut().enumerate() {
                let want = num_at(&out, i * 11 + j + 1);
                c.num(mode, want, got[j], || {
                    format!("bit.{}({x:e}, {y:e}): lua {want} rust {}", names[j], got[j])
                });
            }
        }
        for c in checks {
            c.finish();
        }
    }
}

/// Does lib_math.c `random_seed`'s `d = d * 3.14159265358979323846 + 2.7182818284590452354`
/// (four rounds) give other bits when the C compiler fuses it into one FMA? GCC contracts it
/// on aarch64 (`-ffp-contract=fast` is its default there), so the oracle's vendored LuaJIT
/// seeds differently from PoB's x64 build, which the port follows (CCR-0001).
fn seed_fma_differs(mut d: f64) -> bool {
    let mut f = d;
    for _ in 0..4 {
        d = d * std::f64::consts::PI + std::f64::consts::E;
        f = f.mul_add(std::f64::consts::PI, std::f64::consts::E);
        if d.to_bits() != f.to_bits() {
            return true;
        }
    }
    false
}

/// The draws of a state seeded with the FMA-contracted `random_seed` (lib_math.c, lj_prng.c),
/// to show that a contraction-sensitive seed's mismatch on an aarch64 oracle is only that.
fn fma_seeded_draws(mut d: f64, draws: &[(f64, f64, f64)]) -> Vec<f64> {
    fn step(u: &mut [u64; 4]) -> u64 {
        let mut r = 0u64;
        for (i, (k, q, s)) in [
            (63u32, 31u32, 18u32),
            (58, 19, 28),
            (55, 24, 7),
            (47, 21, 8),
        ]
        .into_iter()
        .enumerate()
        {
            let mut z = u[i];
            z = (((z << q) ^ z) >> (k - s)) ^ ((z & (u64::MAX << (64 - k))) << s);
            r ^= z;
            u[i] = z;
        }
        r
    }
    let mut u = [0u64; 4];
    let mut r: u32 = 0x1109_0601;
    for slot in &mut u {
        let m = 1u64 << (r & 255);
        r >>= 8;
        d = d.mul_add(std::f64::consts::PI, std::f64::consts::E);
        let mut bits = d.to_bits();
        if bits < m {
            bits += m;
        }
        *slot = bits;
    }
    for _ in 0..10 {
        step(&mut u);
    }
    draws
        .iter()
        .map(|&(k, m, n)| {
            let x = f64::from_bits((step(&mut u) & 0x000f_ffff_ffff_ffff) | 0x3ff0_0000_0000_0000)
                - 1.0;
            match k as u8 {
                0 => x,
                1 => (x * m).floor() + 1.0,
                _ => (x * (n - m + 1.0)).floor() + m,
            }
        })
        .collect()
}

/// `math.randomseed` / `math.random` (FUP7: the one port, `m_randomseed` / `m_random`): 64 seeds
/// (integers, fractions, negatives, PoB's `os.time()` range), each followed by 200 draws mixing
/// `random()`, `random(m)` and `random(m, n)`. On an aarch64 host, a seed whose four seeding
/// rounds round differently under FMA ([`seed_fma_differs`]) is compared against the
/// FMA-seeded stream instead (counted as tolerated); every other seed is bit-exact.
#[test]
fn lj_math_random() {
    let mut r = Rng(4242);
    let mut seeds: Vec<f64> = vec![0.0, 1.0, -1.0, 12.5, 1_700_000_000.0, 1e300, -0.0, 0.5];
    while seeds.len() < 64 {
        seeds.push(match r.below(3) {
            0 => r.range(-1_000_000, 2_000_000_000) as f64,
            1 => r.unit() * 1e6 - 5e5,
            _ => random_number(&mut r),
        });
    }
    // (kind, m, n) per draw: 0 = random(), 1 = random(m), 2 = random(m, n)
    let draws: Vec<(f64, f64, f64)> = (0..200)
        .map(|_| {
            let k = r.below(3) as f64;
            let m = r.range(-20, 5000) as f64;
            let n = m + r.range(0, 70_000) as f64;
            (k, m, n)
        })
        .collect();
    let oracle_fuses = cfg!(target_arch = "aarch64");
    for mode in modes() {
        let lua = lua(mode);
        let f = func(
            &lua,
            &format!(
                "{ENC} return function(seeds, K, M, N) local out, j = {{}}, 0 \
                 for s = 1, #seeds do math.randomseed(seeds[s]) \
                 for i = 1, #K do j = j + 1 local k = K[i] \
                 local v if k == 0 then v = math.random() elseif k == 1 then v = math.random(M[i]) \
                 else v = math.random(M[i], N[i]) end E(out, j, v) end end return out end"
            ),
        );
        let ks: Vec<f64> = draws.iter().map(|d| d.0).collect();
        let ms: Vec<f64> = draws.iter().map(|d| d.1).collect();
        let ns: Vec<f64> = draws.iter().map(|d| d.2).collect();
        let out: mlua::Table = f
            .call((
                seq_f64(&lua, &seeds),
                seq_f64(&lua, &ks),
                seq_f64(&lua, &ms),
                seq_f64(&lua, &ns),
            ))
            .unwrap();
        let mut c = Check::new(format!("math.random [{mode}]"));
        let mut exact_seeds = 0;
        let mut j = 0;
        for &seed in &seeds {
            m_randomseed(seed);
            let fused =
                (oracle_fuses && seed_fma_differs(seed)).then(|| fma_seeded_draws(seed, &draws));
            if fused.is_none() {
                exact_seeds += 1;
            }
            for (i, &(k, m, n)) in draws.iter().enumerate() {
                j += 1;
                let got = match k as u8 {
                    0 => m_random(None, None),
                    1 => m_random(Some(m), None),
                    _ => m_random(Some(m), Some(n)),
                };
                let want = num_at(&out, j);
                if let Some(fused) = &fused {
                    c.total += 1;
                    c.profile_tolerated += 1;
                    if !same_num(want, fused[i]) {
                        c.bad.push(format!(
                            "seed {seed:e} draw {i}: lua {want:e} is not the FMA-seeded {:e}",
                            fused[i]
                        ));
                    }
                    continue;
                }
                c.num(mode, want, got, || {
                    format!("seed {seed:e} draw {i} ({k}, {m}, {n}): lua {want:e} rust {got:e}")
                });
            }
        }
        assert!(
            exact_seeds >= 8,
            "only {exact_seeds} seeds compared exactly"
        );
        c.finish();
    }
}
