//! L0 (calc doc §9.4): `pob_lua::pattern` and `pob_lua::string` against
//! LuaJIT: `string.find/match/gmatch/gsub` (string, function and table
//! replacements, error paths) on random patterns, `string.lower/upper/sub/
//! byte/rep`, and PoB's `sanitiseText`, `string:split`, `firstToUpper`,
//! `StripEscapes` run from the real PoB sources.

// Test-only pedantic lints: short binding names mirror the Lua under test.
#![allow(clippy::many_single_char_names, clippy::inefficient_to_string)]

mod common;

use common::*;
use pob_lua::pattern::{
    find_bytes, gmatch_bytes, gsub_bytes_with, match_bytes, Capture, Captures, Repl,
};
use pob_lua::string as ls;
use pob_oracle::mlua::{Table, Value};

const SUBJ: &[u8] = b"aabbxyzAZ_12 ()[]-.%^$\n\xe2";

fn gen_subject(r: &mut Rng) -> Vec<u8> {
    let max = if r.chance(0.1) { 40 } else { 14 };
    let n = r.below(max) as usize;
    (0..n).map(|_| *r.pick(SUBJ)).collect()
}

fn gen_single(r: &mut Rng, ncap: usize) -> Vec<u8> {
    let choices: &[&[u8]] = &[
        b"a",
        b"b",
        b"x",
        b"1",
        b" ",
        b"-",
        b"Z",
        b".",
        b".",
        b"%a",
        b"%d",
        b"%s",
        b"%w",
        b"%l",
        b"%u",
        b"%p",
        b"%x",
        b"%c",
        b"%g",
        b"%z",
        b"%A",
        b"%D",
        b"%S",
        b"%W",
        b"%G",
        b"%(",
        b"%.",
        b"%%",
        b"%-",
        b"%]",
        b"%^",
        b"%$",
        b"[ab]",
        b"[^a-c]",
        b"[%d.]",
        b"[%a_]",
        b"[]]",
        b"[^]]",
        b"[a-]",
        b"[-a]",
        b"[%]x]",
        b"[x-z1]",
        b"[^%s]",
        b"[\x80-\xff]",
        b"\xe2",
    ];
    let mut s = r.pick(choices).to_vec();
    if r.chance(0.4) {
        s.push(*r.pick(b"*+-?"));
    }
    if ncap > 0 && r.chance(0.05) {
        s = format!("%{}", r.range(1, ncap as i64)).into_bytes();
    }
    s
}

fn gen_seq(r: &mut Rng, depth: u32, ncap: &mut usize) -> Vec<u8> {
    let mut p = Vec::new();
    for _ in 0..r.range(0, 4) {
        match r.below(12) {
            0 if depth < 3 => {
                p.push(b'(');
                p.extend(gen_seq(r, depth + 1, ncap));
                p.push(b')');
                *ncap += 1;
            }
            1 => {
                p.extend_from_slice(b"()");
                *ncap += 1;
            }
            2 => p.extend_from_slice(r.pick(&[&b"%b()"[..], b"%b[]", b"%bab"])),
            3 => p.extend_from_slice(r.pick(&[&b"%f[%w]"[..], b"%f[%W]", b"%f[^a]", b"%f[%z]"])),
            _ => p.extend(gen_single(r, *ncap)),
        }
    }
    p
}

fn gen_pattern(r: &mut Rng) -> Vec<u8> {
    let mut ncap = 0;
    let mut p = Vec::new();
    if r.chance(0.2) {
        p.push(b'^');
    }
    p.extend(gen_seq(r, 0, &mut ncap));
    if r.chance(0.15) {
        p.push(b'$');
    }
    if r.chance(0.04) {
        // Malformed patterns.
        p.extend_from_slice(r.pick(&[
            &b"%"[..],
            b"[a",
            b"(",
            b")",
            b"%b",
            b"%fa",
            b"%9",
            b"[^",
            b"[%",
        ]));
    }
    if r.chance(0.01) {
        p = b"(((((((((((((((((((((((((((((((((a)))))))))))))))))))))))))))))))))".to_vec();
        // too many captures
    }
    if r.chance(0.01) {
        p = b"a?".repeat(250); // pattern too complex (LJ_MAX_XLEVEL)
    }
    p
}

/// Serialisation shared with `LUA_SER` (byte-exact).
fn ser_cap(x: &Capture<'_>) -> Vec<u8> {
    match x {
        Capture::Str(b) => ser_bytes(b),
        Capture::Pos(p) => format!("P{p}").into_bytes(),
    }
}

fn ser_caps(c: &Captures<'_>) -> Vec<u8> {
    c.iter().map(ser_cap).collect::<Vec<_>>().join(&b'|')
}

fn ser_bytes(b: &[u8]) -> Vec<u8> {
    let mut v = format!("S{}:", b.len()).into_bytes();
    v.extend_from_slice(b);
    v
}

fn cat(parts: &[&[u8]]) -> Vec<u8> {
    parts.concat()
}

const LUA_SER: &str = r#"
local select, type, tostring, concat, sfmt = select, type, tostring, table.concat, string.format
local function ser(...)
  local n = select('#', ...)
  local t = {}
  for i = 1, n do
    local v = select(i, ...)
    local ty = type(v)
    if ty == "string" then t[i] = "S" .. #v .. ":" .. v
    elseif ty == "number" then t[i] = "P" .. sfmt("%d", v)
    elseif v == nil then t[i] = "N"
    else t[i] = "?" .. tostring(v) end
  end
  return concat(t, "|")
end
local function wrap(ok, ...)
  if ok then return "R" .. ser(...) end
  return "E" .. tostring((...))
end
"#;

fn rs_wrap(f: impl FnOnce() -> Vec<u8>) -> Vec<u8> {
    match rs_catch(f) {
        Ok(s) => cat(&[b"R", &s]),
        Err(e) => cat(&[b"E", e.as_bytes()]),
    }
}

fn same_result(lua: &[u8], rs: &[u8]) -> bool {
    if lua == rs {
        return true;
    }
    match (lua.strip_prefix(b"E"), rs.strip_prefix(b"E")) {
        (Some(l), Some(r)) => {
            lua_msg_matches(&String::from_utf8_lossy(l), &String::from_utf8_lossy(r))
        }
        _ => false,
    }
}

#[test]
fn patterns_find_match_gmatch_gsub() {
    quiet_lua_errors();
    let mut r = Rng(606);
    let n = 15_000;
    let mut subs = Vec::new();
    let mut pats = Vec::new();
    let mut inits: Vec<Option<i64>> = Vec::new();
    let mut plains = Vec::new();
    let mut repls: Vec<Vec<u8>> = Vec::new();
    let mut maxns: Vec<Option<i64>> = Vec::new();
    for _ in 0..n {
        subs.push(gen_subject(&mut r));
        pats.push(gen_pattern(&mut r));
        inits.push(if r.chance(0.6) {
            None
        } else {
            Some(r.range(-30, 30))
        });
        plains.push(r.chance(0.1));
        repls.push(
            r.pick(&[
                &b""[..],
                b"<%0>",
                b"%1",
                b"%2",
                b"[%1|%0]",
                b"%%",
                b"x%a",
                b"%",
                b"-",
            ])
            .to_vec(),
        );
        maxns.push(if r.chance(0.7) {
            None
        } else {
            Some(r.range(-1, 3))
        });
    }
    for mode in modes() {
        let lua = lua(mode);
        let f = func(
            &lua,
            &format!(
                "{LUA_SER} local find, match, gmatch, gsub, pcall = string.find, string.match, string.gmatch, string.gsub, pcall \
                 local up = string.upper \
                 local tbl = {{ a = 'A1', b = false, ['1'] = 'one', [''] = 'E', ab = 'AB' }} \
                 local function fn(c, ...) if type(c) == 'string' and #c % 2 == 1 then return up(c) .. select('#', ...) end return nil end \
                 return function(S, P, I, PL, R, M, n) local out = {{}} for i = 1, n do \
                   local s, p, init, plain, repl, maxn = S[i], P[i], I[i], PL[i], R[i], M[i] \
                   out[7*i-6] = wrap(pcall(find, s, p, init, plain)) \
                   out[7*i-5] = wrap(pcall(match, s, p, init)) \
                   out[7*i-4] = wrap(pcall(function() local acc = {{}} for a, b, c in gmatch(s, p) do \
                       acc[#acc+1] = ser(a, b, c) if #acc > 100 then break end end return concat(acc, '#') end)) \
                   out[7*i-3] = wrap(pcall(gsub, s, p, repl, maxn)) \
                   out[7*i-2] = wrap(pcall(gsub, s, p, fn, maxn)) \
                   out[7*i-1] = wrap(pcall(gsub, s, p, tbl, maxn)) \
                   out[7*i] = wrap(pcall(find, s, p, init, true)) \
                 end return out end"
            ),
        );
        let it = lua.create_table().unwrap();
        let mt = lua.create_table().unwrap();
        for i in 0..n {
            if let Some(v) = inits[i] {
                it.raw_set(i + 1, v).unwrap();
            }
            if let Some(v) = maxns[i] {
                mt.raw_set(i + 1, v).unwrap();
            }
        }
        let pl = lua.create_sequence_from(plains.iter().copied()).unwrap();
        let out: Table = f
            .call((
                seq_bytes(&lua, &subs),
                seq_bytes(&lua, &pats),
                it,
                pl,
                seq_bytes(&lua, &repls),
                mt,
                n,
            ))
            .unwrap();
        let names = [
            "string.find",
            "string.match",
            "string.gmatch",
            "string.gsub(str)",
            "string.gsub(fn)",
            "string.gsub(table)",
            "string.find(plain)",
        ];
        let mut checks: Vec<Check> = names
            .iter()
            .map(|nm| Check::new(format!("{nm} [{mode}]")))
            .collect();
        for i in 0..n {
            let (s, p) = (&subs[i][..], &pats[i][..]);
            let got = [
                rs_wrap(|| match find_bytes(s, p, inits[i], plains[i]) {
                    None => b"N".to_vec(),
                    Some((a, b, c)) => {
                        let mut v =
                            vec![format!("P{a}").into_bytes(), format!("P{b}").into_bytes()];
                        if !c.is_empty() {
                            v.push(ser_caps(&c));
                        }
                        v.join(&b'|')
                    }
                }),
                rs_wrap(|| match match_bytes(s, p, inits[i]) {
                    None => b"N".to_vec(),
                    Some(c) => ser_caps(&c),
                }),
                rs_wrap(|| {
                    let mut acc = Vec::new();
                    for c in gmatch_bytes(s, p) {
                        let mut parts: Vec<Vec<u8>> = c.iter().take(3).map(ser_cap).collect();
                        while parts.len() < 3 {
                            parts.push(b"N".to_vec());
                        }
                        acc.push(parts.join(&b'|'));
                        if acc.len() > 100 {
                            break;
                        }
                    }
                    ser_bytes(&acc.join(&b'#'))
                }),
                rs_wrap(|| {
                    let (b, c) = gsub_bytes_with(s, p, Repl::Str(&repls[i]), maxns[i]);
                    cat(&[&ser_bytes(&b), format!("|P{c}").as_bytes()])
                }),
                rs_wrap(|| {
                    let mut g = |c: &Captures<'_>| match c[0] {
                        Capture::Str(b) if b.len() % 2 == 1 => {
                            let mut v = ls::upper_bytes(b);
                            v.extend_from_slice((c.len() - 1).to_string().as_bytes());
                            Some(v)
                        }
                        _ => None,
                    };
                    let (b, c) = gsub_bytes_with(s, p, Repl::Func(&mut g), maxns[i]);
                    cat(&[&ser_bytes(&b), format!("|P{c}").as_bytes()])
                }),
                rs_wrap(|| {
                    let mut t = |c: &Capture<'_>| match c {
                        Capture::Str(b) => match *b {
                            b"a" => Some(b"A1".to_vec()),
                            b"1" => Some(b"one".to_vec()),
                            b"" => Some(b"E".to_vec()),
                            b"ab" => Some(b"AB".to_vec()),
                            _ => None,
                        },
                        Capture::Pos(_) => None,
                    };
                    let (b, c) = gsub_bytes_with(s, p, Repl::Table(&mut t), maxns[i]);
                    cat(&[&ser_bytes(&b), format!("|P{c}").as_bytes()])
                }),
                rs_wrap(|| match find_bytes(s, p, inits[i], true) {
                    None => b"N".to_vec(),
                    Some((a, b, _)) => format!("P{a}|P{b}").into_bytes(),
                }),
            ];
            for (j, c) in checks.iter_mut().enumerate() {
                let want = bytes_of(&get(&out, 7 * i + j + 1)).unwrap_or_default();
                c.ok(same_result(&want, &got[j]), || {
                    format!(
                        "{}({}, {}, init={:?}, plain={}, repl={}, n={:?}): lua {} rust {}",
                        names[j],
                        show(s),
                        show(p),
                        inits[i],
                        plains[i],
                        show(&repls[i]),
                        maxns[i],
                        show(&want),
                        show(&got[j])
                    )
                });
            }
        }
        for c in checks {
            c.finish();
        }
    }
}

#[test]
fn string_lib_basics() {
    let mut r = Rng(707);
    let n = 20_000;
    let subs: Vec<Vec<u8>> = (0..n)
        .map(|_| {
            (0..r.below(16))
                .map(|_| {
                    if r.chance(0.2) {
                        r.below(256) as u8
                    } else {
                        *r.pick(b"aZq_ 09.-AzM")
                    }
                })
                .collect()
        })
        .collect();
    let is: Vec<i64> = (0..n).map(|_| r.range(-14, 14)).collect();
    let js: Vec<Option<i64>> = (0..n)
        .map(|_| {
            if r.chance(0.3) {
                None
            } else {
                Some(r.range(-14, 14))
            }
        })
        .collect();
    let reps: Vec<i64> = (0..n).map(|_| r.range(-1, 4)).collect();
    // string.sub results of the interpreter run (modes() runs jit.off first).
    // The vendored LuaJIT's compiled traces of `s:sub(i, j)` with variable
    // negative `j` return wrong results (they behave as if j were -1); see
    // the P2 report. Rust follows the interpreter; in jit.on mode a
    // mismatch where Rust equals the interpreter result is counted, not failed.
    let mut interp_sub: Vec<Vec<u8>> = Vec::new();
    for mode in modes() {
        let lua = lua(mode);
        let jt = lua.create_table().unwrap();
        for (k, j) in js.iter().enumerate() {
            if let Some(j) = j {
                jt.raw_set(k + 1, *j).unwrap();
            }
        }
        let f = func(
            &lua,
            &format!(
                "{ENC} return function(S, I, J, RP, n) local out, b = {{}}, {{}} for k = 1, n do local s = S[k] \
                 out[6*k-5] = s:lower() out[6*k-4] = s:upper() \
                 if J[k] == nil then out[6*k-3] = s:sub(I[k]) else out[6*k-3] = s:sub(I[k], J[k]) end \
                 out[6*k-2] = s:rep(RP[k]) out[6*k-1] = s:rep(RP[k], ',') out[6*k] = #s \
                 local v = s:byte(I[k]) if v then E(b, k, v) end end return out, b end"
            ),
        );
        let (out, bt): (Table, Table) = f
            .call((
                seq_bytes(&lua, &subs),
                lua.create_sequence_from(is.iter().map(|&x| x as f64))
                    .unwrap(),
                jt,
                lua.create_sequence_from(reps.iter().map(|&x| x as f64))
                    .unwrap(),
                n,
            ))
            .unwrap();
        let names = [
            "string.lower",
            "string.upper",
            "string.sub",
            "string.rep",
            "string.rep(sep)",
            "#s",
            "string.byte",
        ];
        let mut checks: Vec<Check> = names
            .iter()
            .map(|nm| Check::new(format!("{nm} [{mode}]")))
            .collect();
        for k in 0..n {
            let s = &subs[k];
            let txt = std::str::from_utf8(s).ok();
            let got: [Vec<u8>; 6] = [
                ls::lower_bytes(s),
                ls::upper_bytes(s),
                ls::sub_bytes(s, is[k], js[k]).to_vec(),
                txt.map(|t| ls::rep(t, reps[k].max(0) as usize).into_bytes())
                    .unwrap_or_else(|| s.repeat(reps[k].max(0) as usize)),
                txt.map(|t| ls::rep_sep(t, reps[k], ",").into_bytes())
                    .unwrap_or_default(),
                s.len().to_string().into_bytes(),
            ];
            for j in 0..6 {
                if j == 4 && txt.is_none() {
                    continue; // rep_sep is &str-only
                }
                let want = match get(&out, 6 * k + j + 1) {
                    Value::String(x) => x.as_bytes().to_vec(),
                    v => num_of(&v)
                        .map(|x| (x as i64).to_string().into_bytes())
                        .unwrap_or_default(),
                };
                if j == 2 {
                    if mode == "jit.off" {
                        interp_sub.push(want.clone());
                    } else if want != got[j] && interp_sub.get(k) == Some(&got[j]) {
                        checks[j].total += 1;
                        checks[j].jit_zero_sign += 1;
                        continue;
                    }
                }
                checks[j].ok(want == got[j], || {
                    format!(
                        "{}({}, {}, {:?}, {}): lua {} rust {}",
                        names[j],
                        show(s),
                        is[k],
                        js[k],
                        reps[k],
                        show(&want),
                        show(&got[j])
                    )
                });
            }
            // byte; also the &str wrappers where valid.
            let want = if nil_at(&bt, k + 1) {
                None
            } else {
                Some(num_at(&bt, k + 1) as u8)
            };
            let got = ls::byte_bytes(s, is[k]);
            checks[6].ok(want == got, || {
                format!(
                    "string.byte({}, {}): lua {want:?} rust {got:?}",
                    show(s),
                    is[k]
                )
            });
            if let Some(t) = txt {
                assert_eq!(ls::lower(t).as_bytes(), &ls::lower_bytes(s)[..]);
                assert_eq!(ls::upper(t).as_bytes(), &ls::upper_bytes(s)[..]);
                assert_eq!(
                    ls::sub_lossy(t, is[k], js[k]).as_bytes(),
                    String::from_utf8_lossy(ls::sub_bytes(s, is[k], js[k])).as_bytes()
                );
            }
        }
        for c in checks {
            c.finish();
        }
    }
}

fn pob_string_chunk() -> String {
    format!(
        "{}\n{}\n{}\nlocal function firstToUpper_ok(...) return 1 end\n{}\n{}\n\
         return {{ sanitiseText = sanitiseText, split = string.split, firstToUpper = firstToUpper, StripEscapes = StripEscapes }}",
        pob_lines("Modules/Common.lua", 7, 21),
        pob_lines("Modules/Common.lua", 303, 329),
        pob_lines("Modules/Common.lua", 997, 1011),
        pob_lines("Modules/ModParser.lua", 13, 15),
        pob_lines("_SimpleGraphic.def.lua", 295, 298),
    )
}

#[test]
fn pob_string_helpers() {
    if pob_src().is_none() {
        return; // POB_ALLOW_SKIP=1
    }
    quiet_lua_errors();
    let mut r = Rng(808);
    let n = 14_000;
    let pieces: &[&str] = &[
        "a", "Z", " ", "-", "<", ">", "<b>", "<tag x>", "\u{2010}", "\u{2011}", "\u{2012}",
        "\u{2013}", "\u{2014}", "\u{2015}", "\u{2212}", "\u{e4}", "\u{f6}", "\u{e9}", "\u{fc}",
        "\u{2122}", "^1", "^x12ABef", "^x12", "^", "fire", "cold", ":", ",", "::", "%", "]", ".",
        "1",
    ];
    let latin1: &[u8] = &[0x96, 0x97, 0xe4, 0xf6, 0xe9, 0x80, 0xff];
    let texts: Vec<Vec<u8>> = (0..n)
        .map(|_| {
            let mut v = Vec::new();
            for _ in 0..r.below(8) {
                if r.chance(0.08) {
                    v.push(*r.pick(latin1));
                } else {
                    v.extend_from_slice(r.pick(pieces).as_bytes());
                }
            }
            v
        })
        .collect();
    let seps: Vec<String> = (0..n)
        .map(|_| {
            r.pick(&[":", ",", " ", "-", ".", "a", "%", "]", "^", "::"])
                .to_string()
        })
        .collect();
    for mode in modes() {
        let lua = lua(mode);
        let fns: Table = lua
            .load(pob_string_chunk())
            .eval()
            .expect("PoB string chunk");
        let f = func(
            &lua,
            &format!(
                "{LUA_SER} return function(F, T, SEP, n) local out = {{}} for i = 1, n do local t = T[i] \
                 out[4*i-3] = wrap(pcall(F.sanitiseText, t)) \
                 out[4*i-2] = wrap(pcall(function() return concat(F.split(t, SEP[i]), '\\0') end)) \
                 out[4*i-1] = wrap(pcall(F.firstToUpper, t)) \
                 out[4*i] = wrap(pcall(F.StripEscapes, t)) end return out end"
            ),
        );
        let seps_b: Vec<Vec<u8>> = seps.iter().map(|s| s.as_bytes().to_vec()).collect();
        let out: Table = f
            .call((fns, seq_bytes(&lua, &texts), seq_bytes(&lua, &seps_b), n))
            .unwrap();
        let names = [
            "sanitiseText",
            "string:split",
            "firstToUpper",
            "StripEscapes",
        ];
        let mut checks: Vec<Check> = names
            .iter()
            .map(|nm| Check::new(format!("{nm} [{mode}]")))
            .collect();
        for i in 0..n {
            let t = &texts[i];
            let txt = String::from_utf8(t.clone()).ok();
            let mut got: Vec<Option<Vec<u8>>> =
                vec![Some(rs_wrap(|| ser_bytes(&ls::sanitiseText_bytes(t))))];
            got.push(
                txt.as_ref()
                    .map(|x| rs_wrap(|| ser_bytes(ls::split(x, &seps[i]).join("\0").as_bytes()))),
            );
            got.push(
                txt.as_ref()
                    .map(|x| rs_wrap(|| ser_bytes(ls::firstToUpper(x).as_bytes()))),
            );
            got.push(
                txt.as_ref()
                    .map(|x| rs_wrap(|| ser_bytes(ls::StripEscapes(x).as_bytes()))),
            );
            for (j, c) in checks.iter_mut().enumerate() {
                let Some(g) = &got[j] else { continue }; // &str API: UTF-8 input only
                let want = bytes_of(&get(&out, 4 * i + j + 1)).unwrap_or_default();
                c.ok(same_result(&want, g), || {
                    format!(
                        "{}({}, sep={:?}): lua {} rust {}",
                        names[j],
                        show(t),
                        seps[i],
                        show(&want),
                        show(g)
                    )
                });
            }
        }
        for c in checks {
            c.finish();
        }
    }
}
