//! Shared L0 harness: a LuaJIT state (the vendored LuaJIT of the oracle,
//! through `mlua`), deterministic random vectors, comparison and
//! reporting helpers.
#![allow(dead_code)]

use mlua::{self, Function, Lua, Table, Value};

/// JIT modes the L0 comparisons run in. The reference is the LuaJIT
/// interpreter (`jit.off`, CCR-0001; every oracle parity run uses it too).
/// `L0_JIT_ON=1` adds an informational `jit.on` run, in which documented
/// trace-only divergences are counted, not failed.
pub fn modes() -> Vec<&'static str> {
    let mut m = vec!["jit.off"];
    if std::env::var_os("L0_JIT_ON").is_some_and(|v| v == "1") {
        m.push("jit.on");
    }
    m
}

/// Is the oracle's LuaJIT a dual-number build (any non-x86 `jit.arch`;
/// `lj_arch.h` LJ_ARCH_NUMMODE)? The shipped port follows the x64
/// single-number profile (CCR-0001/0002); on a dual-number host, L0
/// tolerates exactly the mismatches that the ARM64 profile
/// (`pob_lua::profile::with_luajit_arm64_profile`) reproduces.
pub fn host_dualnum() -> bool {
    static D: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *D.get_or_init(|| {
        let arch: String = Lua::new()
            .load("return jit and jit.arch or 'x64'")
            .eval()
            .expect("jit.arch");
        !matches!(arch.as_str(), "x86" | "x64")
    })
}

pub fn lua(mode: &str) -> Lua {
    let lua = Lua::new();
    let code = if mode == "jit.on" {
        "jit.on()"
    } else {
        "jit.off() jit.flush()"
    };
    lua.load(code).exec().expect("jit mode");
    lua
}

pub fn func(lua: &Lua, code: &str) -> Function {
    lua.load(code)
        .eval::<Function>()
        .unwrap_or_else(|e| panic!("lua compile: {e}\n{code}"))
}

/// SplitMix64.
pub struct Rng(pub u64);

impl Rng {
    pub fn next_u64(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9e37_79b9_7f4a_7c15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
        z ^ (z >> 31)
    }
    pub fn below(&mut self, n: u64) -> u64 {
        self.next_u64() % n
    }
    pub fn range(&mut self, lo: i64, hi: i64) -> i64 {
        lo + self.below((hi - lo + 1) as u64) as i64
    }
    pub fn chance(&mut self, p: f64) -> bool {
        (self.next_u64() >> 11) as f64 / (1u64 << 53) as f64 <= p
    }
    pub fn pick<'a, T>(&mut self, xs: &'a [T]) -> &'a T {
        &xs[self.below(xs.len() as u64) as usize]
    }
    pub fn unit(&mut self) -> f64 {
        (self.next_u64() >> 11) as f64 / (1u64 << 53) as f64
    }
}

/// Interesting doubles: zeros, halves, ties, powers, int32/uint32/2^53/2^63
/// boundaries, subnormals, inf/nan, decimal fractions that are not exact.
pub fn special_numbers() -> Vec<f64> {
    let mut v = vec![
        0.0,
        -0.0,
        1.0,
        -1.0,
        0.5,
        -0.5,
        1.5,
        -1.5,
        2.5,
        -2.5,
        3.5,
        0.25,
        0.75,
        0.1,
        0.2,
        0.3,
        -0.1,
        1.0 / 3.0,
        2.0 / 3.0,
        1.005,
        2.675,
        1.45,
        0.045,
        0.0001,
        0.00001,
        1e-5,
        1e-4,
        1e-300,
        1e300,
        -1e300,
        5e-324,
        -5e-324,
        2.2250738585072014e-308,
        2.225073858507201e-308,
        1.7976931348623157e308,
        -1.7976931348623157e308,
        4503599627370496.0,
        4503599627370495.5,
        9007199254740992.0,
        9007199254740993.0,
        9223372036854775808.0,
        -9223372036854775808.0,
        18446744073709551616.0,
        4294967295.0,
        4294967296.0,
        2147483647.0,
        2147483648.0,
        -2147483648.0,
        -2147483649.0,
        2147483647.5,
        -2147483648.5,
        1e14,
        1e15,
        1e16,
        99999999999999.5,
        999999999999999.0,
        123456789012345.0,
        12345678901234.5,
        0.000123456789,
        1e21,
        1e22,
        1e23,
        12345.678,
        100.0,
        1000.0,
        1e6,
        1e-7,
        9.5,
        10.5,
        0.95,
        0.995,
        99.5,
        999.5,
        f64::INFINITY,
        f64::NEG_INFINITY,
        f64::NAN,
        -f64::NAN,
        6755399441055744.0,
        6755399441055743.5,
        std::f64::consts::PI,
        std::f64::consts::E,
        0.30000000000000004,
        1.0000000000000002,
        0.9999999999999999,
    ];
    for k in -20..=20 {
        v.push(f64::from(k) + 0.5);
        v.push(f64::from(k) / 10.0);
        v.push(10f64.powi(k));
        v.push(2f64.powi(k * 3));
    }
    v
}

/// Random doubles of mixed shapes.
pub fn random_number(r: &mut Rng) -> f64 {
    match r.below(10) {
        0 => f64::from_bits(r.next_u64()), // any bit pattern (incl. NaN, inf, subnormal)
        1 => r.range(-1_000_000, 1_000_000) as f64,
        2 => r.range(-100_000, 100_000) as f64 / 10f64.powi(r.range(1, 4) as i32),
        3 => (r.unit() - 0.5) * 10f64.powi(r.range(-30, 30) as i32),
        4 => r.range(-1000, 1000) as f64 + 0.5,
        5 => r.range(-10_000, 10_000) as f64 / 100.0,
        6 => (r.unit() - 0.5) * 2e9,
        7 => r.range(-(1 << 52), 1 << 52) as f64 * 2f64.powi(r.range(-60, 60) as i32),
        8 => r.unit() * 100.0,
        _ => *r.pick(&special_numbers()),
    }
}

/// LuaJIT's C API canonicalises every NaN (NaN-tagged values), so a Lua
/// number never carries a NaN payload; the Rust inputs mirror that.
pub fn canon(x: f64) -> f64 {
    if x.is_nan() {
        f64::from_bits(0xfff8_0000_0000_0000)
    } else {
        x
    }
}

pub fn numbers(seed: u64, n: usize) -> Vec<f64> {
    let mut r = Rng(seed);
    let mut v = special_numbers();
    while v.len() < n {
        v.push(random_number(&mut r));
    }
    v.into_iter().map(canon).collect()
}

/// Bitwise equality with every NaN equal to every NaN (LuaJIT never prints
/// a NaN sign, so the payload is unobservable).
pub fn same_num(a: f64, b: f64) -> bool {
    (a.is_nan() && b.is_nan()) || a.to_bits() == b.to_bits()
}

/// Lua prelude: `E(out, i, x)` stores number `x` losslessly as two exact
/// integers at `out[2i-1], out[2i]` (mlua's number->integer heuristic would
/// turn `-0` and tiny numbers into `0`).
pub const ENC: &str = "local __frexp, __huge = math.frexp, math.huge \
local function E(out, i, x) local k = 2 * i - 1 \
if x ~= x then out[k] = 0 out[k+1] = 4 \
elseif x == __huge then out[k] = 0 out[k+1] = 2 \
elseif x == -__huge then out[k] = 0 out[k+1] = 3 \
elseif x == 0 then out[k] = 0 out[k+1] = (1/x < 0) and 1 or 0 \
else local m, e = __frexp(x) out[k] = m * 9007199254740992 out[k+1] = e end end\n";

/// Decode the number stored by `E(out, i, x)`.
pub fn num_at(t: &Table, i: usize) -> f64 {
    let m = num_of(&get(t, 2 * i - 1)).expect("encoded mantissa");
    let e = num_of(&get(t, 2 * i)).expect("encoded exponent") as i64;
    if m == 0.0 {
        return match e {
            0 => 0.0,
            1 => -0.0,
            2 => f64::INFINITY,
            3 => f64::NEG_INFINITY,
            _ => f64::NAN,
        };
    }
    let neg = m < 0.0;
    let mm = m.abs() as u64; // in [2^52, 2^53)
    assert!((1u64 << 52..1u64 << 53).contains(&mm), "bad mantissa {m}");
    let be = e - 1 + 1023;
    let bits = if be >= 1 {
        ((be as u64) << 52) | (mm & ((1u64 << 52) - 1))
    } else {
        mm >> (1 - be)
    };
    f64::from_bits(bits | if neg { 1u64 << 63 } else { 0 })
}

/// Is `out[i]` (stored with `E`) nil?
pub fn nil_at(t: &Table, i: usize) -> bool {
    matches!(get(t, 2 * i - 1), Value::Nil)
}

pub fn num_of(v: &Value) -> Option<f64> {
    match v {
        Value::Number(n) => Some(*n),
        Value::Integer(i) => Some(*i as f64),
        _ => None,
    }
}

pub fn bytes_of(v: &Value) -> Option<Vec<u8>> {
    match v {
        Value::String(s) => Some(s.as_bytes().to_vec()),
        _ => None,
    }
}

pub fn seq_f64(lua: &Lua, xs: &[f64]) -> Table {
    lua.create_sequence_from(xs.iter().copied()).expect("seq")
}

pub fn seq_bytes(lua: &Lua, xs: &[Vec<u8>]) -> Table {
    let t = lua.create_table().expect("table");
    for (i, x) in xs.iter().enumerate() {
        t.raw_set(i + 1, lua.create_string(x).expect("str"))
            .expect("set");
    }
    t
}

pub fn get(t: &Table, i: usize) -> Value {
    t.raw_get::<Value>(i).expect("get")
}

/// Mismatch collector; panics at the end with the first few mismatches.
pub struct Check {
    pub name: String,
    pub total: usize,
    pub bad: Vec<String>,
    /// Vectors where compiled LuaJIT traces returned a zero of the other
    /// sign than the interpreter (and Rust). LuaJIT's trace narrowing of
    /// `math.floor`/`math.ceil` results to integers drops the sign of zero,
    /// so PoB itself is not deterministic there; Rust follows the
    /// interpreter, which the `jit.off` run checks bit-exactly.
    pub jit_zero_sign: usize,
    /// Vectors where the shipped x64 profile differs from a dual-number
    /// (ARM64) oracle and the ARM64 profile reproduces the oracle exactly
    /// (CCR-0001 table: saturating float->int casts, int32 `tonumber` zeros).
    pub profile_tolerated: usize,
}

impl Check {
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            total: 0,
            bad: Vec::new(),
            jit_zero_sign: 0,
            profile_tolerated: 0,
        }
    }
    pub fn ok(&mut self, cond: bool, what: impl FnOnce() -> String) {
        self.total += 1;
        if !cond {
            self.bad.push(what());
        }
    }
    /// Like [`Self::ok`] for a result that depends on the LuaJIT build
    /// profile: a mismatch is tolerated (and counted) only on a dual-number
    /// host and only when `arm64_same` (the comparison redone under
    /// `with_luajit_arm64_profile`) holds.
    pub fn ok_profile(
        &mut self,
        cond: bool,
        arm64_same: impl FnOnce() -> bool,
        what: impl FnOnce() -> String,
    ) {
        self.total += 1;
        if cond {
            return;
        }
        if host_dualnum() && pob_lua::profile::with_luajit_arm64_profile(arm64_same) {
            self.profile_tolerated += 1;
            return;
        }
        self.bad.push(what());
    }
    /// Compare numbers: bit-exact (NaN == NaN), except that in `jit.on`
    /// mode a zero of the other sign is tolerated and counted. This applies
    /// to EVERY `jit.on` comparison made through `num`, not only to the
    /// functions whose traces are known to lose the zero sign; the `jit.off`
    /// run of the same vectors is always bit-exact, so the interpreter
    /// reference is still fully checked.
    pub fn num(&mut self, mode: &str, want: f64, got: f64, what: impl FnOnce() -> String) {
        if mode == "jit.on" && want == 0.0 && got == 0.0 && want.to_bits() != got.to_bits() {
            self.total += 1;
            self.jit_zero_sign += 1;
            return;
        }
        self.ok(same_num(want, got), what);
    }
    pub fn finish(self) -> usize {
        eprintln!(
            "L0 {:<40} {:>7} vectors, {:>4} mismatches{}{}",
            self.name,
            self.total,
            self.bad.len(),
            if self.jit_zero_sign > 0 {
                format!(
                    " ({} JIT-only divergences tolerated: trace result differs from interpreter)",
                    self.jit_zero_sign
                )
            } else {
                String::new()
            },
            if self.profile_tolerated > 0 {
                format!(
                    " ({} x64-vs-ARM64 LuaJIT profile differences tolerated, each reproduced by the ARM64 profile)",
                    self.profile_tolerated
                )
            } else {
                String::new()
            }
        );
        if !self.bad.is_empty() {
            let shown: Vec<_> = self.bad.iter().take(25).cloned().collect();
            panic!(
                "{}: {} of {} vectors differ from LuaJIT:\n{}",
                self.name,
                self.bad.len(),
                self.total,
                shown.join("\n")
            );
        }
        self.total
    }
}

/// Run `f` and turn a `lua_error!` into `Err(message)`.
pub fn rs_catch<R>(f: impl FnOnce() -> R) -> Result<R, String> {
    pob_lua::error::catch(f).map_err(|e| e.0)
}

/// Install a panic hook that stays quiet for `LuaError` payloads (expected
/// in error-path vectors), keeping the default output for real panics.
pub fn quiet_lua_errors() {
    use std::sync::Once;
    static ONCE: Once = Once::new();
    ONCE.call_once(|| {
        let prev = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |info| {
            if info.payload().downcast_ref::<pob_lua::LuaError>().is_none() {
                prev(info);
            }
        }));
    });
}

/// LuaJIT prefixes errors raised from C functions with the caller position
/// (`[string "..."]:3: `); compare messages without it.
/// LuaJIT names the function in "bad argument" errors after the call site
/// (`'?'` through `pcall`), so function names are compared as wildcards.
pub fn lua_msg_matches(lua_msg: &str, rs_msg: &str) -> bool {
    fn norm(m: &str) -> String {
        match (m.find(" to '"), m.find("' (")) {
            (Some(a), Some(b)) if m.starts_with("bad argument") && b > a => {
                format!("{} to '?{}", &m[..a], &m[b..])
            }
            _ => m.to_owned(),
        }
    }
    let (l, r) = (norm(lua_msg), norm(rs_msg));
    l == r || l.ends_with(&format!(": {r}"))
}

/// PoB's `src/` for the L0 tests that quote PoB source (e.g.
/// `string_format_calc_literals`), found by `pob_testpaths` (`$POB_SRC`, this
/// workspace and its ancestors, the linked worktree's main checkout, the main
/// checkout's absolute path). A missing PoB checkout fails the test; `None`
/// only with `POB_ALLOW_SKIP=1` (the test then returns).
pub fn pob_src() -> Option<std::path::PathBuf> {
    pob_testpaths::require(
        pob_testpaths::pob_src(),
        "PoB checkout (.PathOfBuilding/src) for L0",
    )
}

/// Lines `a..=b` (1-based) of a PoB source file. The test checked
/// [`pob_src`] first.
pub fn pob_lines(rel: &str, a: usize, b: usize) -> String {
    let src = pob_testpaths::pob_src().expect("PoB checkout (check pob_src() first)");
    let text = std::fs::read(src.join(rel)).expect("read PoB file");
    let text = String::from_utf8_lossy(&text);
    text.split('\n')
        .skip(a - 1)
        .take(b + 1 - a)
        .collect::<Vec<_>>()
        .join("\n")
}

pub fn show(b: &[u8]) -> String {
    format!("{:?}", String::from_utf8_lossy(b))
}

pub type LuaResult<T> = mlua::Result<T>;
