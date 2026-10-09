//! L0 (calc doc §9.4): `pob_lua::format` against LuaJIT: `tostring(number)`,
//! `..`, `string.format` (generated specs + every `s_format`/`string.format`
//! literal in `Modules/Calc*.lua`), `tonumber(s)` and `tonumber(s, base)`.

// Test-only pedantic lints: short binding names mirror the Lua under test.
#![allow(
    clippy::many_single_char_names,
    clippy::inefficient_to_string,
    clippy::naive_bytecount,
    clippy::case_sensitive_file_extension_comparisons
)]

mod common;

use common::*;
use pob_lua::format::{
    lua_format_bytes, tonumber_base_bytes, tonumber_bytes, tostring_num, FmtArg,
};
use pob_oracle::mlua::{Lua, Table, Value};

#[derive(Clone, Debug)]
enum Arg {
    Num(f64),
    Str(String),
    Bool(bool),
    Nil,
}

impl Arg {
    fn fmt_arg(&self) -> FmtArg<'_> {
        match self {
            Arg::Num(n) => FmtArg::Num(*n),
            Arg::Str(s) => FmtArg::Str(s),
            Arg::Bool(b) => FmtArg::Bool(*b),
            Arg::Nil => FmtArg::Nil,
        }
    }
    fn lua(&self, lua: &Lua) -> Value {
        match self {
            Arg::Num(n) => Value::Number(*n),
            Arg::Str(s) => Value::String(lua.create_string(s).unwrap()),
            Arg::Bool(b) => Value::Boolean(*b),
            Arg::Nil => Value::Nil,
        }
    }
}

/// Run `string.format(fmts[i], args[i]...)` under pcall in LuaJIT and compare
/// with `lua_format_bytes`.
fn run_format_vectors(name: &str, fmts: &[Vec<u8>], args: &[Vec<Arg>]) {
    quiet_lua_errors();
    for mode in modes() {
        let lua = lua(mode);
        let argt = lua.create_table().unwrap();
        for (i, a) in args.iter().enumerate() {
            let t = lua.create_table().unwrap();
            for (j, v) in a.iter().enumerate() {
                t.raw_set(j + 1, v.lua(&lua)).unwrap();
            }
            t.raw_set("n", a.len()).unwrap();
            argt.raw_set(i + 1, t).unwrap();
        }
        let f = func(
            &lua,
            "local fmt, pcall, unpack = string.format, pcall, unpack \
             return function(fmts, args) local out = {} for i = 1, #fmts do local a = args[i] \
             local ok, r = pcall(fmt, fmts[i], unpack(a, 1, a.n)) out[2*i-1] = ok out[2*i] = r end return out end",
        );
        let out: Table = f.call((seq_bytes(&lua, fmts), argt)).unwrap();
        let mut c = Check::new(format!("{name} [{mode}]"));
        for i in 0..fmts.len() {
            let ok = matches!(get(&out, 2 * i + 1), Value::Boolean(true));
            let r = bytes_of(&get(&out, 2 * i + 2)).unwrap_or_default();
            let fa: Vec<FmtArg> = args[i].iter().map(Arg::fmt_arg).collect();
            let run = || rs_catch(|| lua_format_bytes(&fmts[i], &fa));
            let cmp = |got: &Result<Vec<u8>, String>| match (got, ok) {
                (Ok(g), true) => *g == r,
                (Err(m), false) => lua_msg_matches(&String::from_utf8_lossy(&r), m),
                _ => false,
            };
            let got = run();
            // `%d %i %o %u %x %X %c` casts depend on the LuaJIT build profile (CCR-0001).
            c.ok_profile(cmp(&got), || cmp(&run()), || {
                format!(
                    "format({}, {:?}): lua {}{} rust {:?}",
                    show(&fmts[i]),
                    args[i],
                    if ok { "" } else { "ERROR " },
                    show(&r),
                    got.as_ref()
                        .map(|g| String::from_utf8_lossy(g).into_owned())
                )
            });
        }
        c.finish();
    }
}

#[test]
fn tostring_and_concat() {
    for mode in modes() {
        let lua = lua(mode);
        let xs = numbers(101, 30_000);
        let f = func(
            &lua,
            "local ts = tostring return function(xs) local out = {} for i = 1, #xs do local x = xs[i] \
             out[2*i-1] = ts(x) out[2*i] = x .. '' end return out end",
        );
        let out: Table = f.call(seq_f64(&lua, &xs)).unwrap();
        let mut c1 = Check::new(format!("tostring(number) [{mode}]"));
        let mut c2 = Check::new(format!("number .. '' / cat! [{mode}]"));
        for (i, &x) in xs.iter().enumerate() {
            let want = bytes_of(&get(&out, 2 * i + 1)).unwrap();
            let got = tostring_num(x);
            c1.ok(want == got.as_bytes(), || {
                format!(
                    "tostring({x:e} / {:#x}): lua {} rust {got:?}",
                    x.to_bits(),
                    show(&want)
                )
            });
            let want2 = bytes_of(&get(&out, 2 * i + 2)).unwrap();
            let got2 = pob_lua::cat!(x);
            c2.ok(want2 == got2.as_str().as_bytes(), || {
                format!("{x:e} .. '': lua {} rust {got2:?}", show(&want2))
            });
        }
        c1.finish();
        c2.finish();
    }
}

const CONVS: &[u8] = b"dioxXucfeEgGaAsqp";

fn gen_spec(r: &mut Rng) -> (Vec<u8>, u8) {
    let mut s = vec![b'%'];
    for _ in 0..r.below(4) {
        s.push(*r.pick(b"-+ #0"));
    }
    match r.below(4) {
        0 => {}
        1 => s.extend_from_slice(r.range(1, 9).to_string().as_bytes()),
        2 => s.extend_from_slice(r.range(10, 40).to_string().as_bytes()),
        _ => s.extend_from_slice(r.range(0, 99).to_string().as_bytes()),
    }
    match r.below(5) {
        0 | 1 => {}
        2 => s.push(b'.'),
        3 => s.extend_from_slice(format!(".{}", r.range(0, 20)).as_bytes()),
        _ => s.extend_from_slice(format!(".{}", r.range(0, 99)).as_bytes()),
    }
    let conv = if r.chance(0.01) {
        *r.pick(b"yzbkl%3")
    } else {
        *r.pick(CONVS)
    };
    s.push(conv);
    (s, conv)
}

fn gen_text(r: &mut Rng) -> String {
    let n = r.below(8) as usize;
    (0..n)
        .map(|_| {
            *r.pick(&[
                'a', ' ', 'Z', '(', '^', '8', '\n', '"', '\\', '\u{1}', '0', '9', '%', 'é',
            ])
        })
        .map(|c| {
            if c == '%' {
                "%%".to_owned()
            } else {
                c.to_string()
            }
        })
        .collect()
}

fn gen_arg_for(conv: u8, r: &mut Rng) -> Arg {
    // `%p` of a string prints its address: not reproducible (pob-lua raises
    // a LuaError instead), so `%p` only gets non-GC values.
    if r.chance(0.02) && conv != b'p' {
        return match r.below(4) {
            0 => Arg::Nil,
            1 => Arg::Bool(r.chance(0.5)),
            2 => Arg::Str("abc".into()),
            _ => Arg::Str(" 0x1A ".into()),
        };
    }
    match conv {
        b's' | b'q' => match r.below(4) {
            0 => Arg::Num(canon(random_number(r))),
            1 => Arg::Bool(r.chance(0.5)),
            _ => Arg::Str(gen_text(r) + &gen_text(r)),
        },
        b'c' => Arg::Num(if r.chance(0.9) {
            r.range(32, 126) as f64
        } else {
            canon(random_number(r))
        }),
        b'p' => match r.below(3) {
            0 => Arg::Num(1.0),
            1 => Arg::Bool(true),
            _ => Arg::Nil,
        },
        _ => {
            if r.chance(0.05) {
                Arg::Str(
                    r.pick(&["12", " 7 ", "1e3", "0x10", "-2.5", "  -0  "])
                        .to_string(),
                )
            } else {
                Arg::Num(canon(random_number(r)))
            }
        }
    }
}

#[test]
fn string_format_generated() {
    let mut r = Rng(202);
    let mut fmts = Vec::new();
    let mut args = Vec::new();
    // Every conversion with ≥ 10k vectors overall, plus flag/width/precision mixes.
    while fmts.len() < 120_000 {
        let mut f = gen_text(&mut r).into_bytes();
        let mut a = Vec::new();
        for _ in 0..r.range(1, 3) {
            let (spec, conv) = gen_spec(&mut r);
            f.extend_from_slice(&spec);
            f.extend_from_slice(gen_text(&mut r).as_bytes());
            if conv != b'%' && !r.chance(0.01) {
                a.push(gen_arg_for(conv, &mut r));
            }
        }
        fmts.push(f);
        args.push(a);
    }
    // Fixed edge cases.
    for (f, a) in [
        ("%", vec![]),
        ("%5", vec![Arg::Num(1.0)]),
        ("%100d", vec![Arg::Num(1.0)]),
        ("%.100f", vec![Arg::Num(1.0)]),
        ("%d", vec![]),
        ("%d %d", vec![Arg::Num(1.0)]),
        (
            "%5.2s|%-5s|%.0s",
            vec![
                Arg::Str("abcdef".into()),
                Arg::Str("ab".into()),
                Arg::Str("x".into()),
            ],
        ),
        ("%.14g", vec![Arg::Num(123456789012345.0)]),
        ("%.99f", vec![Arg::Num(1e308)]),
        ("%.99e", vec![Arg::Num(5e-324)]),
        (
            "%#.0f %#x %#o %#g",
            vec![Arg::Num(1.0), Arg::Num(0.0), Arg::Num(0.0), Arg::Num(1.0)],
        ),
        ("%x", vec![Arg::Num(-1.0)]),
        ("%u", vec![Arg::Num(-1.0)]),
        ("%d", vec![Arg::Num(f64::NAN)]),
        ("%d", vec![Arg::Num(1e30)]),
        ("%c", vec![Arg::Num(65.5)]),
        ("%q", vec![Arg::Str("\u{0}1\u{7f}\u{1}2\r\n\"\\".into())]),
    ] {
        fmts.push(f.as_bytes().to_vec());
        args.push(a);
    }
    // `%p` of a string prints an address (not reproducible; pob-lua raises
    // instead): drop vectors where a skipped argument shifted a string onto `%p`.
    let keep: Vec<bool> = fmts
        .iter()
        .zip(&args)
        .map(|(f, a)| {
            !conversions(f)
                .iter()
                .zip(a)
                .any(|(c, v)| *c == b'p' && matches!(v, Arg::Str(_)))
        })
        .collect();
    let dropped = keep.iter().filter(|k| !**k).count();
    let fmts: Vec<_> = fmts
        .into_iter()
        .zip(&keep)
        .filter(|(_, k)| **k)
        .map(|(f, _)| f)
        .collect();
    let args: Vec<_> = args
        .into_iter()
        .zip(&keep)
        .filter(|(_, k)| **k)
        .map(|(a, _)| a)
        .collect();
    eprintln!("  dropped {dropped} vectors with a string under %p");
    run_format_vectors("string.format generated", &fmts, &args);
    // Per-conversion counts (each ≥ 10k).
    for conv in CONVS {
        let n: usize = fmts
            .iter()
            .map(|f| conversions(f).iter().filter(|c| *c == conv).count())
            .sum();
        eprintln!("  '%{}' conversions tested: {n}", *conv as char);
        assert!(n >= 10_000, "fewer than 10k vectors for %{}", *conv as char);
    }
}

/// The conversion characters of a format, in argument order.
fn conversions(f: &[u8]) -> Vec<u8> {
    let mut convs = Vec::new();
    let mut i = 0;
    while i < f.len() {
        if f[i] == b'%' {
            if f.get(i + 1) == Some(&b'%') {
                i += 2;
                continue;
            }
            let mut j = i + 1;
            while j < f.len() && b"-+ #0123456789.".contains(&f[j]) {
                j += 1;
            }
            convs.push(*f.get(j).unwrap_or(&b'?'));
            i = j + 1;
        } else {
            i += 1;
        }
    }
    convs
}

/// Every `s_format("...")` / `string.format("...")` literal in Modules/Calc*.lua.
fn calc_format_literals(src: &std::path::Path) -> Vec<String> {
    let dir = src.join("Modules");
    let mut lits = std::collections::BTreeSet::new();
    for e in std::fs::read_dir(&dir).unwrap() {
        let p = e.unwrap().path();
        let name = p.file_name().unwrap().to_string_lossy().into_owned();
        if !(name.starts_with("Calc") && name.ends_with(".lua")) {
            continue;
        }
        let text = String::from_utf8_lossy(&std::fs::read(&p).unwrap()).into_owned();
        for pat in ["s_format(\"", "string.format(\""] {
            let mut rest = text.as_str();
            while let Some(k) = rest.find(pat) {
                let after = &rest[k + pat.len()..];
                let bytes = after.as_bytes();
                let mut j = 0;
                while j < bytes.len() && bytes[j] != b'"' {
                    if bytes[j] == b'\\' {
                        j += 1;
                    }
                    j += 1;
                }
                lits.insert(after[..j].to_owned());
                rest = &after[j..];
            }
        }
    }
    lits.into_iter().collect()
}

#[test]
fn string_format_calc_literals() {
    let Some(src) = pob_src() else { return };
    let lits = calc_format_literals(&src);
    assert!(lits.len() > 600, "found only {} literals", lits.len());
    // Let LuaJIT parse the literals' escapes.
    let lua0 = lua("jit.off");
    let src = format!(
        "return {{ {} }}",
        lits.iter()
            .map(|l| format!("\"{l}\""))
            .collect::<Vec<_>>()
            .join(", ")
    );
    let t: Table = lua0.load(src).eval().unwrap();
    let parsed: Vec<Vec<u8>> = (1..=lits.len())
        .map(|i| bytes_of(&get(&t, i)).unwrap())
        .collect();
    let mut r = Rng(303);
    let mut fmts = Vec::new();
    let mut args = Vec::new();
    for f in &parsed {
        let convs = conversions(f);
        for _ in 0..20 {
            fmts.push(f.clone());
            args.push(
                convs
                    .iter()
                    .map(|&cv| match cv {
                        b's' => {
                            if r.chance(0.5) {
                                Arg::Str(gen_text(&mut r))
                            } else {
                                Arg::Num(canon(random_number(&mut r)))
                            }
                        }
                        _ => Arg::Num(canon(random_number(&mut r))),
                    })
                    .collect(),
            );
        }
    }
    eprintln!("  {} literals x 20 argument sets", parsed.len());
    run_format_vectors("s_format literals in Calc*.lua", &fmts, &args);
}

fn gen_numeric_string(r: &mut Rng) -> Vec<u8> {
    let mut s = Vec::new();
    let sp = |r: &mut Rng, s: &mut Vec<u8>| {
        for _ in 0..r.below(3) {
            s.push(*r.pick(b" \t\n\r\x0b\x0c"));
        }
    };
    if r.chance(0.3) {
        sp(r, &mut s);
    }
    if r.chance(0.3) {
        s.push(*r.pick(b"+-"));
    }
    match r.below(12) {
        0 => s.extend_from_slice(r.pick(&[
            &b"inf"[..],
            b"INF",
            b"infinity",
            b"nan",
            b"NaN",
            b"infx",
            b"in",
            b"nana",
        ])),
        1 => {
            s.extend_from_slice(r.pick(&[&b"0x"[..], b"0X"]));
            for _ in 0..r.below(20) {
                s.push(*r.pick(b"0123456789abcdefABCDEF"));
            }
            if r.chance(0.3) {
                s.push(b'.');
                for _ in 0..r.below(6) {
                    s.push(*r.pick(b"0123456789abcdef"));
                }
            }
            if r.chance(0.3) {
                s.push(*r.pick(b"pP"));
                if r.chance(0.5) {
                    s.push(*r.pick(b"+-"));
                }
                s.extend_from_slice(r.range(0, 1100).to_string().as_bytes());
            }
        }
        2 => {
            s.extend_from_slice(b"0b");
            for _ in 0..r.below(70) {
                s.push(*r.pick(b"01"));
            }
        }
        3 => {
            // Long decimal strings (beyond 20 / 800 digits).
            for _ in 0..r.range(15, 900) {
                s.push(*r.pick(b"0123456789"));
            }
            if r.chance(0.5) {
                s.push(b'.');
                for _ in 0..r.below(40) {
                    s.push(*r.pick(b"0123456789"));
                }
            }
        }
        4 => s.extend_from_slice(r.pick(&[
            &b""[..],
            b".",
            b"e5",
            b"1e",
            b"1e+",
            b".e1",
            b"1..2",
            b"1.2.3",
            b"12u",
            b"12ll",
            b"1i",
            b"0x",
            b"0b2",
            b"--1",
            b"1 2",
            b"1\x002",
        ])),
        _ => {
            // Ordinary decimals with optional fraction and exponent.
            for _ in 0..r.range(0, 12) {
                s.push(*r.pick(b"0123456789"));
            }
            if r.chance(0.5) {
                s.push(b'.');
                for _ in 0..r.range(0, 12) {
                    s.push(*r.pick(b"0123456789"));
                }
            }
            if r.chance(0.4) {
                s.push(*r.pick(b"eE"));
                if r.chance(0.5) {
                    s.push(*r.pick(b"+-"));
                }
                s.extend_from_slice(r.range(0, 400).to_string().as_bytes());
            }
        }
    }
    if r.chance(0.3) {
        sp(r, &mut s);
    }
    if r.chance(0.03) {
        s.push(*r.pick(b"xZ!_"));
    }
    s
}

#[test]
fn tonumber_strings() {
    let mut r = Rng(404);
    let mut strs: Vec<Vec<u8>> = (0..30_000).map(|_| gen_numeric_string(&mut r)).collect();
    // Round-trip strings of random doubles in several formats.
    for x in numbers(405, 10_000) {
        strs.push(
            match r.below(4) {
                0 => format!("{x:e}"),
                1 => format!("{x:?}"),
                2 => format!("{:.20e}", x),
                _ => tostring_num(x),
            }
            .into_bytes(),
        );
    }
    for mode in modes() {
        let lua = lua(mode);
        let f = func(&lua, &format!("{ENC} local tn = tonumber return function(xs) local out = {{}} for i = 1, #xs do local v = tn(xs[i]) if v ~= nil then E(out, i, v) end end return out end"));
        let out: Table = f.call(seq_bytes(&lua, &strs)).unwrap();
        let mut c = Check::new(format!("tonumber(s) [{mode}]"));
        for (i, s) in strs.iter().enumerate() {
            let want = if nil_at(&out, i + 1) {
                None
            } else {
                Some(num_at(&out, i + 1))
            };
            let cmp = |got: Option<f64>| match (want, got) {
                (None, None) => true,
                (Some(a), Some(b)) => same_num(a, b),
                _ => false,
            };
            let got = tonumber_bytes(s);
            // Dual-number builds scan "-0" / "-0b0" as integer +0 (CCR-0001).
            c.ok_profile(cmp(got), || cmp(tonumber_bytes(s)), || {
                format!("tonumber({}): lua {want:?} rust {got:?}", show(s))
            });
        }
        c.finish();
    }
}

#[test]
fn tonumber_with_base() {
    quiet_lua_errors();
    let mut r = Rng(505);
    let mut strs = Vec::new();
    let mut bases = Vec::new();
    for _ in 0..30_000 {
        let base = r.range(2, 36) as u32;
        let mut s = Vec::new();
        if r.chance(0.2) {
            s.push(*r.pick(b" \t"));
        }
        if r.chance(0.2) {
            s.push(*r.pick(b"+-"));
        }
        if base == 16 && r.chance(0.3) {
            s.extend_from_slice(b"0x");
        }
        let nd = if r.chance(0.1) { 30 } else { 8 };
        for _ in 0..r.below(nd) {
            s.push(*r.pick(b"0123456789abcdefghijklmnopqrstuvwxyzABCDEFXYZ"));
        }
        if r.chance(0.1) {
            s.push(*r.pick(b" .-x"));
        }
        strs.push(s);
        bases.push(if r.chance(0.002) {
            *r.pick(&[0u32, 1, 37, 10])
        } else {
            base
        });
    }
    for mode in modes() {
        let lua = lua(mode);
        let f = func(
            &lua,
            &format!("{ENC} local tn = tonumber return function(xs, bs) local out = {{}} local err = {{}} for i = 1, #xs do \
             local ok, v = pcall(tn, xs[i], bs[i]) if not ok then err[i] = v elseif v ~= nil then E(out, i, v) end end return out, err end"),
        );
        let bt = lua
            .create_sequence_from(bases.iter().map(|&b| f64::from(b)))
            .unwrap();
        let (out, err): (Table, Table) = f.call((seq_bytes(&lua, &strs), bt)).unwrap();
        let mut c = Check::new(format!("tonumber(s, base) [{mode}]"));
        for (i, s) in strs.iter().enumerate() {
            let lerr = bytes_of(&get(&err, i + 1));
            let want = if nil_at(&out, i + 1) {
                None
            } else {
                Some(num_at(&out, i + 1))
            };
            let run = || rs_catch(|| tonumber_base_bytes(s, bases[i]));
            let cmp = |got: &Result<Option<f64>, String>| match (&lerr, got) {
                (Some(m), Err(e)) => lua_msg_matches(&String::from_utf8_lossy(m), e),
                (None, Ok(g)) => match (want, *g) {
                    (None, None) => true,
                    (Some(a), Some(b)) => same_num(a, b),
                    _ => false,
                },
                _ => false,
            };
            let got = run();
            // Dual-number builds return "-0" in any base as integer +0 (CCR-0001).
            c.ok_profile(cmp(&got), || cmp(&run()), || {
                format!(
                    "tonumber({}, {}): lua {want:?} err {:?} rust {got:?}",
                    show(s),
                    bases[i],
                    lerr.as_ref().map(|m| show(m))
                )
            });
        }
        c.finish();
    }
}
