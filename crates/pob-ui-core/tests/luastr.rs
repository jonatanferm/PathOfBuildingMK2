//! U5 (ui doc §12.3, §11.1 T1): `pob_ui_core::luastr` against the real implementations.
//!
//! * LuaJIT (the vendored LuaJIT of the oracle, through mlua; interpreter, `jit.off`, like every
//!   oracle run): `string.sub` byte semantics with Lua-number indices on UTF-8 input, the
//!   EditControl concatenations (`buf:sub(1, l - 1) .. text .. buf:sub(r)`), the
//!   expression-position `gsub`/`match` forms, `tostring` and `..` of numbers.
//! * lua-utf8 (luautf8 bdd3d7f, PoB's `runtime/lua-utf8.dll` source) and PoB's `formatNumSep`:
//!   the checked-in vectors `luastr_data/utf8_vectors.txt`, produced by
//!   `luastr_data/gen_utf8_vectors.lua` with the real C module (it cannot be loaded into the
//!   vendored LuaJIT; the generator header says how to regenerate).
//! * The §8 audit: every helper path named in `luastr.rs`'s resolution table (compile-time).
//!
//! Rule 4 (ui doc §8): where a Lua result is not valid UTF-8, the Rust `&str`/`String` form is the
//! lossy re-validation of Lua's bytes; such vectors are counted separately ("lossy"), every other
//! vector must be byte-identical.

#![allow(
    clippy::too_many_lines,
    clippy::many_single_char_names,
    clippy::single_match_else,
    clippy::format_push_string,
    clippy::type_complexity
)]

use mlua::{Function, Lua, Value};
use pob_ui_core::luastr::{self, utf8 as u8s, LBuf};

// ---------------------------------------------------------------------------------------------
// Harness
// ---------------------------------------------------------------------------------------------

fn lua() -> Lua {
    let lua = Lua::new();
    lua.load("jit.off() jit.flush()").exec().expect("jit.off");
    lua
}

fn func(lua: &Lua, code: &str) -> Function {
    lua.load(code)
        .eval::<Function>()
        .unwrap_or_else(|e| panic!("lua compile: {e}\n{code}"))
}

/// Is the LuaJIT a dual-number (non-x86) build? Its float->int casts saturate (ARM64 `fcvtzs`)
/// where the shipped x64 profile gives MIN (CCR-0001); vectors whose only difference is that
/// cast are tolerated (and counted) on such a host.
fn host_dualnum(lua: &Lua) -> bool {
    let arch: String = lua
        .load("return jit and jit.arch or 'x64'")
        .eval()
        .expect("jit.arch");
    !matches!(arch.as_str(), "x86" | "x64")
}

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
    fn chance(&mut self, p: f64) -> bool {
        (self.next_u64() >> 11) as f64 / (1u64 << 53) as f64 <= p
    }
    fn pick<'a, T>(&mut self, xs: &'a [T]) -> &'a T {
        &xs[self.below(xs.len() as u64) as usize]
    }
}

/// Text pieces: ASCII, 2/3/4-byte characters, a combining mark.
const PIECES: &[&str] = &[
    "a",
    "b",
    "Z",
    "0",
    "7",
    " ",
    "\t",
    "\n",
    "\r",
    ".",
    "-",
    "%",
    "^",
    "(",
    ")",
    "[",
    "]",
    "\u{e9}",
    "\u{f6}",
    "\u{20ac}",
    "\u{4e2d}",
    "\u{1f600}",
    "\u{301}",
    "\u{a0}",
];
/// Invalid UTF-8 byte pieces (Lua strings are bytes).
const BAD: &[&[u8]] = &[b"\xc3", b"\x80", b"\xff", b"\xe2\x82"];

fn text(r: &mut Rng, max: u64) -> String {
    (0..r.below(max + 1)).map(|_| *r.pick(PIECES)).collect()
}

fn bytes(r: &mut Rng, max: u64) -> Vec<u8> {
    let mut v = Vec::new();
    for _ in 0..r.below(max + 1) {
        if r.chance(0.08) {
            v.extend_from_slice(r.pick(BAD));
        } else {
            v.extend_from_slice(r.pick(PIECES).as_bytes());
        }
    }
    v
}

fn show(b: &[u8]) -> String {
    format!("{:?}", String::from_utf8_lossy(b))
}

fn lua_bytes(v: &Value) -> Option<Vec<u8>> {
    match v {
        Value::String(s) => Some(s.as_bytes().to_vec()),
        _ => None,
    }
}

/// Mismatch collector; panics at the end with the first mismatches.
struct Check {
    name: String,
    total: usize,
    lossy: usize,
    tolerated: usize,
    bad: Vec<String>,
}

impl Check {
    fn new(name: &str) -> Self {
        Self {
            name: name.into(),
            total: 0,
            lossy: 0,
            tolerated: 0,
            bad: Vec::new(),
        }
    }
    fn ok(&mut self, cond: bool, what: impl FnOnce() -> String) {
        self.total += 1;
        if !cond {
            self.bad.push(what());
        }
    }
    /// Compare a Rust `String` result with Lua's bytes under rule 4: exact when Lua's bytes are
    /// UTF-8, otherwise the lossy re-validation.
    fn text(&mut self, want: &[u8], got: &str, what: impl FnOnce() -> String) {
        match std::str::from_utf8(want) {
            Ok(w) => self.ok(w == got, what),
            Err(_) => {
                self.lossy += 1;
                self.ok(String::from_utf8_lossy(want) == got, what);
            }
        }
    }
    fn finish(self) {
        eprintln!(
            "U5 {:<36} {:>6} vectors, {:>3} mismatches ({} lossy-by-rule-4, {} host-cast tolerated)",
            self.name,
            self.total,
            self.bad.len(),
            self.lossy,
            self.tolerated
        );
        assert!(
            self.bad.is_empty(),
            "{}: {} of {} vectors differ:\n{}",
            self.name,
            self.bad.len(),
            self.total,
            self.bad
                .iter()
                .take(12)
                .cloned()
                .collect::<Vec<_>>()
                .join("\n")
        );
    }
}

/// Index arguments: integers around a short string, fractions, signed zero, and values whose
/// `lj_num2int` is the x64 "integer indefinite" (NaN, ±inf, outside int32).
const IDX: &[f64] = &[
    -12.0,
    -9.0,
    -6.0,
    -4.0,
    -3.0,
    -2.0,
    -1.0,
    0.0,
    1.0,
    2.0,
    3.0,
    4.0,
    5.0,
    7.0,
    9.0,
    12.0,
    0.5,
    -0.5,
    2.5,
    2.9,
    -2.5,
    -2.9,
    -0.0,
    1.999_999,
    2_147_483_647.0,
    -2_147_483_648.0,
    2_147_483_648.0,
    -2_147_483_649.0,
    3e9,
    -3e9,
    1e300,
    -1e300,
    f64::INFINITY,
    f64::NEG_INFINITY,
    f64::NAN,
];

fn x64_cast_sensitive(x: f64) -> bool {
    !(x.is_finite() && x > -2_147_483_649.0 && x < 2_147_483_648.0)
}

/// `pob_lua::string::sub_bytes` with ARM64's saturating casts (`fcvtzs`; NaN -> 0).
fn sub_arm64(s: &[u8], i: f64, j: Option<f64>) -> Vec<u8> {
    pob_lua::string::sub_bytes(s, i64::from(i as i32), j.map(|j| i64::from(j as i32))).to_vec()
}

// ---------------------------------------------------------------------------------------------
// LuaJIT: string.sub with Lua-number indices
// ---------------------------------------------------------------------------------------------

#[test]
fn sub_with_number_indices_vs_luajit() {
    let lua = lua();
    let dual = host_dualnum(&lua);
    let f = func(
        &lua,
        "return function(s, i, j) if j == nil then return s:sub(i) end return s:sub(i, j) end",
    );
    let mut r = Rng(0x55);
    let mut c_bytes = Check::new("string.sub (bytes, f64 idx)");
    let mut c_str = Check::new("string.sub (&str, f64 idx)");
    for k in 0..12_000 {
        let s = if k % 3 == 0 {
            text(&mut r, 8).into_bytes()
        } else {
            bytes(&mut r, 8)
        };
        let i = *r.pick(IDX);
        let j = if r.chance(0.25) {
            None
        } else {
            Some(*r.pick(IDX))
        };
        let ls = lua.create_string(&s).unwrap();
        let want: Vec<u8> = lua_bytes(&f.call::<Value>((ls, i, j)).unwrap()).unwrap();
        let got = luastr::sub_bytes(&s, i, j);
        let what = || {
            format!(
                "sub({}, {i}, {j:?}): lua {} rust {}",
                show(&s),
                show(&want),
                show(got)
            )
        };
        if want != got
            && dual
            && (x64_cast_sensitive(i) || j.is_some_and(x64_cast_sensitive))
            && want == sub_arm64(&s, i, j)
        {
            c_bytes.total += 1;
            c_bytes.tolerated += 1;
            continue;
        }
        c_bytes.ok(want == got, what);
        if let Ok(st) = std::str::from_utf8(&s) {
            let got = luastr::sub(st, i, j);
            c_str.text(&want, &got, || {
                format!("sub({st:?}, {i}, {j:?}): lua {} rust {got:?}", show(&want))
            });
        }
    }
    c_bytes.finish();
    c_str.finish();
}

// ---------------------------------------------------------------------------------------------
// LuaJIT: EditControl concatenations
// ---------------------------------------------------------------------------------------------

#[test]
fn splice_and_insert_vs_luajit() {
    let lua = lua();
    let f = func(
        &lua,
        "return function(buf, l, r, t) local s = buf:sub(1, l - 1) .. t .. buf:sub(r) return s, #s end",
    );
    let mut r = Rng(0x0059_11ce);
    let mut c_splice = Check::new("buf:sub(1,l-1)..t..buf:sub(r)");
    let mut c_insert = Check::new("buf:sub(1,c-1)..t..buf:sub(c)");
    let mut c_lbuf = Check::new("LBuf # (byte length)");
    for k in 0..12_000 {
        let buf = text(&mut r, 9);
        let t = if r.chance(0.3) {
            String::new()
        } else {
            text(&mut r, 3)
        };
        let n = buf.len() as f64;
        let pos = |r: &mut Rng| -> f64 {
            if r.chance(0.1) {
                *r.pick(&[0.5, 1.5, -1.0, n + 3.0, 2.5])
            } else {
                r.below(buf.len() as u64 + 3) as f64
            }
        };
        let (l, rr) = if k % 2 == 0 {
            let c = pos(&mut r);
            (c, c)
        } else {
            let a = pos(&mut r);
            let b = pos(&mut r);
            (a.min(b), a.max(b))
        };
        let (s, len): (Value, f64) = f.call((buf.as_str(), l, rr, t.as_str())).unwrap();
        let want = lua_bytes(&s).unwrap();
        if k % 2 == 0 {
            let got = luastr::insert(&buf, l, &t);
            c_insert.text(&want, &got, || {
                format!(
                    "insert({buf:?}, {l}, {t:?}): lua {} rust {got:?}",
                    show(&want)
                )
            });
        } else {
            let got = luastr::splice(&buf, l, rr, &t);
            c_splice.text(&want, &got, || {
                format!(
                    "splice({buf:?}, {l}, {rr}, {t:?}): lua {} rust {got:?}",
                    show(&want)
                )
            });
        }
        let mut b = LBuf::new();
        b.push_sub(&buf, 1.0, Some(l - 1.0))
            .push_str(&t)
            .push_sub(&buf, rr, None);
        c_lbuf.ok(b.len() == len && b.0 == want, || {
            format!(
                "LBuf({buf:?}, {l}, {rr}, {t:?}): lua {} (#{len}) rust {:?}",
                show(&want),
                b.0
            )
        });
    }
    c_splice.finish();
    c_insert.finish();
    c_lbuf.finish();
}

// ---------------------------------------------------------------------------------------------
// LuaJIT: expression-position gsub / match (byte patterns on UTF-8 text)
// ---------------------------------------------------------------------------------------------

const PATS: &[&str] = &[
    "[\u{80}-\u{ff}]", // as UTF-8 text: "[" C2 80 "-" C3 BF "]" (a byte set over those bytes)
    "\r",
    "%s+",
    "^%s*(.-)%s*$",
    "()",
    "\u{e9}",
    "[\u{e9}]",
    ".",
    "(%w+)",
    "%^x%x%x%x%x%x%x",
    "(%d+)",
    "%c",
    "[^%w%p ]",
    "(.)(.)",
    "%p+$",
    "\n()",
];
const BYTE_PATS: &[&[u8]] = &[b"[\x80-\xff]", b"[\xc3]", b"\xa9", b"[\x80-\xbf]+"];
const REPLS: &[&str] = &["", "?", "%1", "<%0>", "%%", "\u{20ac}", "%x"];

#[test]
fn expression_forms_vs_luajit() {
    let lua = lua();
    let g = func(&lua, "return function(s, p, r) return (s:gsub(p, r)) end");
    let m = func(
        &lua,
        "return function(s, p) local v = s:match(p) if v == nil then return nil end return tostring(v) end",
    );
    let mut r = Rng(0xe4);
    let mut c_g = Check::new("(s:gsub(p, r)) first value");
    let mut c_m = Check::new("s:match(p) first value");
    for k in 0..8_000 {
        let s = text(&mut r, 10);
        let p: String = if k % 5 == 0 {
            // a byte-level pattern that is not UTF-8 text (EditControl.lua:539 "[\128-\255]")
            let pb = *r.pick(BYTE_PATS);
            let rp = *r.pick(REPLS);
            let lp = lua.create_string(pb).unwrap();
            let want = lua_bytes(&g.call::<Value>((s.as_str(), lp, rp)).unwrap()).unwrap();
            let got = luastr::gsub_bytes_pat(&s, pb, rp);
            c_g.text(&want, &got, || {
                format!(
                    "gsub_bytes_pat({s:?}, {}, {rp:?}): lua {} rust {got:?}",
                    show(pb),
                    show(&want)
                )
            });
            continue;
        } else {
            (*r.pick(PATS)).to_owned()
        };
        let rp = *r.pick(REPLS);
        let want = lua_bytes(&g.call::<Value>((s.as_str(), p.as_str(), rp)).unwrap()).unwrap();
        let got = luastr::gsub(&s, &p, rp);
        c_g.text(&want, &got, || {
            format!(
                "gsub({s:?}, {p:?}, {rp:?}): lua {} rust {got:?}",
                show(&want)
            )
        });
        let want = lua_bytes(&m.call::<Value>((s.as_str(), p.as_str())).unwrap());
        let got = luastr::lmatch(&s, &p);
        match (&want, &got) {
            (Some(w), Some(gt)) => {
                c_m.text(w, gt, || {
                    format!("match({s:?}, {p:?}): lua {} rust {gt:?}", show(w))
                });
            }
            _ => c_m.ok(want.is_none() && got.is_none(), || {
                format!("match({s:?}, {p:?}): lua {want:?} rust {got:?}")
            }),
        }
    }
    c_g.finish();
    c_m.finish();
}

// ---------------------------------------------------------------------------------------------
// LuaJIT: tostring and `..` of numbers
// ---------------------------------------------------------------------------------------------

#[test]
fn tostring_and_concat_vs_luajit() {
    let lua = lua();
    let dual = host_dualnum(&lua);
    let f = func(
        &lua,
        "return function(n) return tostring(n), 'x' .. n .. 'y', n .. '' end",
    );
    let mut nums: Vec<f64> = vec![
        0.0,
        -0.0,
        1.0,
        -1.0,
        0.1,
        0.5,
        1e14,
        1e15,
        1e16,
        123_456_789_012_345.0,
        1.0 / 3.0,
        f64::INFINITY,
        f64::NEG_INFINITY,
        f64::NAN,
        2.5,
        -2.5,
        1e-5,
        1e300,
        5e-324,
        99.95,
        1234.5,
        4_294_967_296.0,
        2_147_483_648.0,
    ];
    let mut r = Rng(0x7057);
    for _ in 0..4_000 {
        let x = f64::from_bits(r.next_u64());
        nums.push(if x.is_nan() { f64::NAN } else { x });
        nums.push(
            (r.below(2_000_000) as f64 - 1_000_000.0) / f64::from(10u32.pow(r.below(5) as u32)),
        );
    }
    let mut c = Check::new("tostring / .. / lcat! / LBuf");
    for &n in &nums {
        let (a, b, cc): (Value, Value, Value) = f.call(n).unwrap();
        let (a, b, cc) = (
            lua_bytes(&a).unwrap(),
            lua_bytes(&b).unwrap(),
            lua_bytes(&cc).unwrap(),
        );
        let ga = luastr::tostring(&n);
        let gb = pob_ui_core::lcat!("x", n, "y");
        let mut lb = LBuf::new();
        lb.push_num(n);
        let same = a == ga.as_bytes() && b == gb.as_bytes() && cc == lb.0;
        // An ARM64 (dual-number) LuaJIT has no -0 for integer-valued results (CCR-0002).
        if !same && dual && n == 0.0 && a == b"0" && ga == "-0" {
            c.total += 1;
            c.tolerated += 1;
            continue;
        }
        c.ok(same, || {
            format!("{n:?}: lua {} {} rust {ga} {gb}", show(&a), show(&b))
        });
    }
    c.finish();
}

// ---------------------------------------------------------------------------------------------
// lua-utf8 and formatNumSep: oracle vectors from the real C module
// ---------------------------------------------------------------------------------------------

const VECTORS: &str = include_str!("luastr_data/utf8_vectors.txt");

fn unesc(s: &str) -> Vec<u8> {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'\\' && b.get(i + 1) == Some(&b'x') {
            out.push(u8::from_str_radix(&s[i + 2..i + 4], 16).expect("hex escape"));
            i += 4;
        } else {
            out.push(b[i]);
            i += 1;
        }
    }
    out
}

fn esc(b: &[u8]) -> String {
    let mut s = String::new();
    for &c in b {
        if (33..=126).contains(&c) && c != b'\\' {
            s.push(c as char);
        } else {
            s.push_str(&format!("\\x{c:02X}"));
        }
    }
    s
}

fn num(n: f64) -> String {
    format!("n:{}", pob_lua::lformat!("%.17g", n))
}

fn cap(c: &pob_lua::pattern::Capture<'_>) -> String {
    match c {
        pob_lua::pattern::Capture::Str(b) => format!("s:{}", esc(b)),
        pob_lua::pattern::Capture::Pos(p) => num(*p as f64),
    }
}

#[derive(Debug)]
enum Arg {
    S(Vec<u8>),
    N(f64),
    B(bool),
    Nil,
}

impl Arg {
    fn parse(s: &str) -> Arg {
        if let Some(x) = s.strip_prefix("s:") {
            Arg::S(unesc(x))
        } else if let Some(x) = s.strip_prefix("n:") {
            Arg::N(x.parse().expect("number"))
        } else if s == "true" || s == "false" {
            Arg::B(s == "true")
        } else {
            assert_eq!(s, "nil", "bad arg {s}");
            Arg::Nil
        }
    }
    fn s(&self) -> &[u8] {
        match self {
            Arg::S(b) => b,
            a => panic!("expected string, got {a:?}"),
        }
    }
    fn n(&self) -> Option<f64> {
        match self {
            Arg::N(x) => Some(*x),
            Arg::Nil => None,
            a => panic!("expected number, got {a:?}"),
        }
    }
    fn truthy(&self) -> bool {
        !matches!(self, Arg::Nil | Arg::B(false))
    }
}

/// The Lua error message without the chunk position the generator's Lua frames add
/// (`[string "..."]:N: `); luautf8 errors raised directly from pcall carry none.
fn strip_pos(msg: &str) -> &str {
    if msg.starts_with("[string \"") {
        if let Some(i) = msg.find("\"]:") {
            let rest = &msg[i + 3..];
            if let Some(j) = rest.find(": ") {
                return &rest[j + 2..];
            }
        }
    }
    msg
}

fn run_vector(f: &str, a: &[Arg]) -> Vec<String> {
    match f {
        "sub" => {
            let j = a.get(2).and_then(Arg::n);
            vec![format!(
                "s:{}",
                esc(u8s::sub(a[0].s(), a[1].n().unwrap(), j))
            )]
        }
        "reverse" => {
            let lax = a.get(1).is_some_and(Arg::truthy);
            vec![format!("s:{}", esc(&u8s::reverse(a[0].s(), lax)))]
        }
        "next" => match u8s::next(
            a[0].s(),
            a.get(1).and_then(Arg::n),
            a.get(2).and_then(Arg::n),
        ) {
            Some((p, c)) => vec![num(p), num(c)],
            None => vec![],
        },
        "find" => {
            let init = a.get(2).and_then(Arg::n);
            let plain = a.get(3).is_some_and(Arg::truthy);
            match u8s::find(a[0].s(), a[1].s(), init, plain) {
                Some((s, e, caps)) => {
                    let mut v = vec![num(s), num(e)];
                    v.extend(caps.iter().map(cap));
                    v
                }
                None => vec!["nil".into()],
            }
        }
        "match" => match u8s::r#match(a[0].s(), a[1].s(), a.get(2).and_then(Arg::n)) {
            Some(caps) => caps.iter().map(cap).collect(),
            None => vec!["nil".into()],
        },
        "gsub" => {
            let (s, n) = u8s::gsub(a[0].s(), a[1].s(), a[2].s(), a.get(3).and_then(Arg::n));
            vec![format!("s:{}", esc(&s)), num(n)]
        }
        "fns" => {
            let s = std::str::from_utf8(a[0].s()).expect("formatNumSep input is text");
            let show = a[1].truthy();
            let sep = std::str::from_utf8(a[2].s()).expect("separator is text");
            let dec = std::str::from_utf8(a[3].s()).expect("separator is text");
            let (out, n) = luastr::format_num_sep(s, show, sep, dec);
            vec![format!("s:{}", esc(out.as_bytes())), num(n as f64)]
        }
        _ => panic!("unknown vector fn {f}"),
    }
}

/// The vectors raise hundreds of expected Lua errors (caught with `pob_lua::error::catch`);
/// keep the default panic hook quiet for those payloads only.
fn quiet_lua_errors() {
    static ONCE: std::sync::Once = std::sync::Once::new();
    ONCE.call_once(|| {
        let prev = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |info| {
            if info.payload().downcast_ref::<pob_lua::LuaError>().is_none() {
                prev(info);
            }
        }));
    });
}

#[test]
fn utf8_and_format_num_sep_vs_lua_utf8_vectors() {
    quiet_lua_errors();
    let mut checks: std::collections::BTreeMap<&str, Check> = std::collections::BTreeMap::new();
    let mut errors = 0usize;
    for (ln, line) in VECTORS.lines().enumerate() {
        let fields: Vec<&str> = line.split('\t').collect();
        let arrow = fields.iter().position(|f| *f == "=>").expect("=>");
        let f = fields[0];
        let args: Vec<Arg> = fields[1..arrow].iter().map(|s| Arg::parse(s)).collect();
        let want_fields = &fields[arrow + 1..];
        let got = pob_lua::error::catch(|| run_vector(f, &args));
        let check = checks
            .entry(f)
            .or_insert_with(|| Check::new(&format!("utf8 vectors: {f}")));
        match (want_fields.first(), got) {
            (Some(&"!err"), Err(e)) => {
                errors += 1;
                let want = String::from_utf8_lossy(&unesc(want_fields[1])).into_owned();
                check.ok(strip_pos(&want) == e.0, || {
                    format!(
                        "line {}: {line}\n   error: lua {want:?} rust {:?}",
                        ln + 1,
                        e.0
                    )
                });
            }
            (_, got) => {
                let got = match got {
                    Ok(v) => v,
                    Err(e) => vec!["!err".into(), esc(e.0.as_bytes())],
                };
                check.ok(want_fields == got.as_slice(), || {
                    format!("line {}: {line}\n   rust: {}", ln + 1, got.join("\t"))
                });
            }
        }
    }
    let total: usize = checks.values().map(|c| c.total).sum();
    eprintln!("U5 utf8 vectors: {total} total, {errors} of them Lua errors matched by message");
    assert!(total >= 6000, "fixture too small: {total}");
    for (_, c) in checks {
        c.finish();
    }
}

// ---------------------------------------------------------------------------------------------
// Shipped x64 number-argument semantics (not observable on an ARM64 oracle host)
// ---------------------------------------------------------------------------------------------

#[test]
fn x64_number_arguments() {
    // lj_num2int: x64 cvttsd2si r32 gives MIN for NaN/±inf/out of int32, so `s:sub(1, 1e10)` is
    // `s:sub(1, -2147483648)` = "" and `s:sub(1e10)` is the whole string.
    assert_eq!(luastr::sub("hello", 1.0, Some(1e10)), "");
    assert_eq!(luastr::sub("hello", 1e10, None), "hello");
    assert_eq!(luastr::sub("hello", f64::NAN, Some(2.0)), "he");
    // lua_tointeger (64-bit): luautf8 sees i64::MIN for NaN, then CAST(int, ..) = 0.
    assert_eq!(u8s::sub(b"hello", f64::NAN, None), b"hello");
    assert_eq!(
        u8s::next(b"hello", Some(f64::NAN), Some(1.0)),
        Some((1.0, 104.0))
    );
}

// ---------------------------------------------------------------------------------------------
// §8 audit: every helper path of the luastr.rs resolution table exists with the expected shape.
// ---------------------------------------------------------------------------------------------

#[test]
fn audit_resolves() {
    use pob_lua::math;
    let _: fn(f64) -> f64 = math::m_floor;
    let _: fn(f64) -> f64 = math::m_ceil;
    let _: fn(f64, f64) -> f64 = math::m_min;
    let _: fn(f64, f64) -> f64 = math::m_max;
    let _: fn(&[f64]) -> f64 = math::m_min_n;
    let _: fn(&[f64]) -> f64 = math::m_max_n;
    let _: fn(f64) -> f64 = math::m_abs;
    let _: fn(f64) -> f64 = math::m_sqrt;
    let _: fn(f64) -> (f64, f64) = math::m_modf;
    let _: fn(f64, Option<f64>) -> f64 = math::m_log;
    let _: fn(f64, f64) -> f64 = math::m_pow;
    let _: fn(f64, f64) -> f64 = math::lpow;
    let _: (f64, f64) = (math::m_huge, math::m_pi);
    let _: fn(f64) -> f64 = f64::sin;
    let _: fn(f64) -> f64 = f64::to_radians;
    let _: fn(f64, f64) -> f64 = pob_lua::fmod_lua;
    let _: fn(f64, Option<i32>) -> f64 = pob_lua::round;
    let _: fn(u32, u32) -> u32 = pob_lua::bit::band;
    let _: fn(u32, u32) -> u32 = pob_lua::bit::bor;
    let _: fn(u32, u32) -> u32 = pob_lua::bit::bxor;
    let _: fn(u32) -> u32 = pob_lua::bit::bnot;
    let _: fn(u32, u32) -> u32 = pob_lua::bit::lshift;
    let _: fn(u32, u32) -> u32 = pob_lua::bit::rshift;
    let _: fn(f64) -> i32 = pob_lua::bit::tobit;
    let _: fn(f64) -> String = pob_lua::tostring;
    let _: fn(&str) -> Option<f64> = pob_lua::tonumber;
    let _: fn(&str, u32) -> Option<f64> = pob_lua::tonumber_base;
    let _: String = pob_lua::lformat!("%d", 1.0);
    let _: fn(&str, &str, &str, Option<i64>) -> (String, usize) = pob_lua::gsub;
    let _ = pob_lua::gsub_fn("a", "a", |_| None, None);
    let _ = pob_lua::gsub_table("a", "a", |_| None, None);
    let _ = pob_lua::lmatch("a", "a", None);
    let _ = pob_lua::find("a", "a", None, false);
    let _ = pob_lua::gmatch("a", "a");
    let _: fn(&str, i64, Option<i64>) -> std::borrow::Cow<'_, str> = pob_lua::string::sub;
    let _: fn(&str) -> String = pob_lua::string::lower;
    let _: fn(&str) -> String = pob_lua::string::upper;
    let _: fn(&str, usize) -> String = pob_lua::string::rep;
    let _: fn(&str, i64, &str) -> String = pob_lua::string::rep_sep;
    let _: fn(&str, i64) -> Option<u8> = pob_lua::string::byte;
    let _: fn(&[u8]) -> Vec<u8> = pob_lua::string::char_bytes;
    let _: fn(&[u8]) -> Vec<u8> = pob_lua::string::reverse_bytes;
    let _: fn(&str) -> String = pob_lua::string::StripEscapes;
    let _: fn(&str) -> String = pob_lua::string::sanitiseText;
    let _: fn(&str, &str) -> Vec<String> = pob_lua::string::split;
    let mut v = vec![3.0, 1.0];
    pob_lua::sort(&mut v, |a, b| a < b);
    let _ = pob_lua::LjIntMap::<u8>::new();
    pob_lua::ipairs::ipairs_live(&mut vec![Some(1u8)], |_, _, _| {});
    let r = pob_lua::error::catch(|| pob_ui_core::lua_error!("x"));
    assert!(r.is_err());
    // luastr's own entries
    let _: fn(&str, &str, &str) -> String = luastr::gsub;
    let _: fn(&str, &str) -> Option<String> = luastr::lmatch;
    let _: fn(&str, &[u8], &str) -> String = luastr::gsub_bytes_pat;
    let _: fn(f64) -> i32 = luastr::lua_int;
    let _: fn(f64) -> i64 = luastr::lua_integer;
    let _: fn(&str) -> f64 = luastr::len;
    let _: fn(&str, f64, Option<f64>) -> std::borrow::Cow<'_, str> = luastr::sub;
    let _: fn(&[u8], f64, Option<f64>) -> &[u8] = luastr::sub_bytes;
    let _: fn(&str, f64, f64, &str) -> String = luastr::splice;
    let _: fn(&str, f64, &str) -> String = luastr::insert;
    let _: fn(&f64) -> String = luastr::tostring::<f64>;
    let _: String = pob_ui_core::lcat!("a", 1.0);
    let _: fn(&str, bool, &str, &str) -> (String, usize) = luastr::format_num_sep;
    let _: fn(&pob_ui_core::main_core::MainCore, &str) -> String = luastr::format_num_sep_main;
    let _: fn(&[u8], f64, Option<f64>) -> &[u8] = u8s::sub;
    let _: fn(&[u8], bool) -> Vec<u8> = u8s::reverse;
    let _: fn(&[u8], Option<f64>, Option<f64>) -> Option<(f64, f64)> = u8s::next;
    let _ = u8s::find(b"a", b"a", None, false);
    let _ = u8s::r#match(b"a", b"a", None);
    let _: fn(&[u8], &[u8], &[u8], Option<f64>) -> (Vec<u8>, f64) = u8s::gsub;
}
