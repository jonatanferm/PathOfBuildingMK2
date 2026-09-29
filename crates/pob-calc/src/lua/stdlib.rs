//! The subset of Lua 5.1's standard library (+ LuaJIT `bit`) PoB code uses.

use std::cell::RefCell;
use std::rc::Rc;

use super::interp::{lt, Interp, LuaError};
use super::value::{num_to_string, TableRef, Val};
use crate::lua_pattern::{self as pat, Capture};

type R<T> = Result<T, LuaError>;

fn arg(args: &[Val], i: usize) -> Val {
    args.get(i).cloned().unwrap_or_default()
}

fn arg_str(args: &[Val], i: usize, fname: &str) -> R<String> {
    match args.get(i) {
        Some(Val::Str(s)) => Ok(s.to_string()),
        Some(Val::Num(n)) => Ok(num_to_string(*n)),
        other => Err(LuaError::msg(format!(
            "bad argument #{} to '{fname}' (string expected, got {})",
            i + 1,
            other.map_or("no value", Val::type_name)
        ))),
    }
}

fn arg_num(args: &[Val], i: usize, fname: &str) -> R<f64> {
    args.get(i).and_then(Val::to_num).ok_or_else(|| {
        LuaError::msg(format!(
            "bad argument #{} to '{fname}' (number expected, got {})",
            i + 1,
            args.get(i).map_or("no value", Val::type_name)
        ))
    })
}

fn opt_num(args: &[Val], i: usize) -> Option<f64> {
    args.get(i).and_then(Val::to_num)
}

fn arg_table(args: &[Val], i: usize, fname: &str) -> R<TableRef> {
    match args.get(i) {
        Some(Val::Table(t)) => Ok(t.clone()),
        other => Err(LuaError::msg(format!(
            "bad argument #{} to '{fname}' (table expected, got {})",
            i + 1,
            other.map_or("no value", Val::type_name)
        ))),
    }
}

fn caps_to_vals(caps: &[Capture<'_>]) -> Vec<Val> {
    caps.iter()
        .map(|c| match c {
            Capture::Str(s) => Val::str(s),
            Capture::Pos(p) => Val::Num(*p as f64),
        })
        .collect()
}

fn pat_err(e: pat::PatternError) -> LuaError {
    LuaError::msg(e.0)
}

/// Lua `tostring`.
#[must_use]
pub fn tostring(v: &Val) -> String {
    v.to_string()
}

fn reg(t: &TableRef, name: &str, f: impl Fn(&mut Interp, Vec<Val>) -> R<Vec<Val>> + 'static) {
    t.set_str(name, Val::native(f));
}

#[allow(clippy::too_many_lines)]
pub fn install(it: &mut Interp) {
    let g = it.globals.clone();

    // ---- base ----
    reg(&g, "type", |_, a| {
        Ok(vec![Val::str(arg(&a, 0).type_name())])
    });
    reg(&g, "tostring", |_, a| {
        Ok(vec![Val::str(&tostring(&arg(&a, 0)))])
    });
    reg(&g, "tonumber", |_, a| {
        let v = arg(&a, 0);
        let base = opt_num(&a, 1);
        Ok(vec![match (&v, base) {
            (Val::Num(n), None) => Val::Num(*n),
            (Val::Str(s), None) => pat::to_number(s).map_or(Val::Nil, Val::Num),
            (Val::Str(s), Some(b)) => {
                i64::from_str_radix(s.trim(), b as u32).map_or(Val::Nil, |n| Val::Num(n as f64))
            }
            _ => Val::Nil,
        }])
    });
    reg(&g, "print", |_, a| {
        let s: Vec<String> = a.iter().map(tostring).collect();
        eprintln!("{}", s.join("\t"));
        Ok(Vec::new())
    });
    reg(&g, "error", |_, a| {
        Err(LuaError::msg(tostring(&arg(&a, 0))))
    });
    reg(&g, "assert", |_, a| {
        if arg(&a, 0).truthy() {
            Ok(a)
        } else {
            Err(LuaError::msg(
                a.get(1)
                    .map_or_else(|| "assertion failed!".to_owned(), tostring),
            ))
        }
    });
    reg(&g, "pcall", |it, mut a| {
        if a.is_empty() {
            return Err(LuaError::msg("bad argument #1 to 'pcall'"));
        }
        let f = a.remove(0);
        match it.call(&f, a) {
            Ok(mut v) => {
                v.insert(0, Val::Bool(true));
                Ok(v)
            }
            Err(e) => Ok(vec![Val::Bool(false), Val::str(&e.0)]),
        }
    });
    reg(&g, "select", |_, a| {
        let n = arg(&a, 0);
        if n.as_str() == Some("#") {
            return Ok(vec![Val::Num((a.len() - 1) as f64)]);
        }
        let i = n
            .to_num()
            .ok_or_else(|| LuaError::msg("bad argument #1 to 'select'"))? as i64;
        let rest = &a[1..];
        let start = if i < 0 {
            (rest.len() as i64 + i).max(0) as usize
        } else {
            (i - 1).max(0) as usize
        };
        Ok(rest.get(start..).map(<[Val]>::to_vec).unwrap_or_default())
    });
    let unpack = |_: &mut Interp, a: Vec<Val>| -> R<Vec<Val>> {
        let t = arg_table(&a, 0, "unpack")?;
        let i = opt_num(&a, 1).unwrap_or(1.0) as i64;
        let j = opt_num(&a, 2).map_or(t.len() as i64, |n| n as i64);
        Ok((i..=j).map(|k| t.get_int(k)).collect())
    };
    reg(&g, "unpack", unpack);
    reg(&g, "rawget", |_, a| {
        Ok(vec![arg_table(&a, 0, "rawget")?.get(&arg(&a, 1))])
    });
    reg(&g, "rawset", |_, a| {
        arg_table(&a, 0, "rawset")?.set(arg(&a, 1), arg(&a, 2))?;
        Ok(vec![arg(&a, 0)])
    });
    reg(&g, "rawequal", |_, a| {
        Ok(vec![Val::Bool(arg(&a, 0).raw_eq(&arg(&a, 1)))])
    });
    reg(&g, "setmetatable", |_, a| Ok(vec![arg(&a, 0)]));
    let next = |_: &mut Interp, a: Vec<Val>| -> R<Vec<Val>> {
        let t = arg_table(&a, 0, "next")?;
        Ok(match t.next(&arg(&a, 1))? {
            Some((k, v)) => vec![k, v],
            None => vec![Val::Nil],
        })
    };
    reg(&g, "next", next);
    let next_fn = g.get_str("next");
    reg(&g, "pairs", move |_, a| {
        let t = arg_table(&a, 0, "pairs")?;
        Ok(vec![next_fn.clone(), Val::Table(t), Val::Nil])
    });
    reg(&g, "ipairs", |_, a| {
        let t = arg_table(&a, 0, "ipairs")?;
        let iter = Val::native(|_, a| {
            let t = arg_table(&a, 0, "ipairs")?;
            let i = arg_num(&a, 1, "ipairs")? as i64 + 1;
            let v = t.get_int(i);
            Ok(if v.is_nil() {
                vec![Val::Nil]
            } else {
                vec![Val::Num(i as f64), v]
            })
        });
        Ok(vec![iter, Val::Table(t), Val::Num(0.0)])
    });

    // ---- string ----
    let s = it.string_lib.clone();
    reg(&s, "lower", |_, a| {
        Ok(vec![Val::str(
            &arg_str(&a, 0, "lower")?.to_ascii_lowercase(),
        )])
    });
    reg(&s, "upper", |_, a| {
        Ok(vec![Val::str(
            &arg_str(&a, 0, "upper")?.to_ascii_uppercase(),
        )])
    });
    reg(&s, "len", |_, a| {
        Ok(vec![Val::Num(arg_str(&a, 0, "len")?.len() as f64)])
    });
    reg(&s, "rep", |_, a| {
        let st = arg_str(&a, 0, "rep")?;
        let n = arg_num(&a, 1, "rep")?.max(0.0) as usize;
        Ok(vec![Val::str(&st.repeat(n))])
    });
    reg(&s, "reverse", |_, a| {
        Ok(vec![Val::str(
            &arg_str(&a, 0, "reverse")?.chars().rev().collect::<String>(),
        )])
    });
    reg(&s, "sub", |_, a| {
        let st = arg_str(&a, 0, "sub")?;
        let l = st.len() as i64;
        let norm = |i: i64| if i < 0 { l + i + 1 } else { i };
        let mut i = norm(opt_num(&a, 1).unwrap_or(1.0) as i64);
        let mut j = norm(opt_num(&a, 2).unwrap_or(-1.0) as i64);
        if i < 1 {
            i = 1;
        }
        if j > l {
            j = l;
        }
        let out = if i > j {
            String::new()
        } else {
            String::from_utf8_lossy(&st.as_bytes()[(i - 1) as usize..j as usize]).into_owned()
        };
        Ok(vec![Val::str(&out)])
    });
    reg(&s, "byte", |_, a| {
        let st = arg_str(&a, 0, "byte")?;
        let i = opt_num(&a, 1).unwrap_or(1.0) as i64;
        let j = opt_num(&a, 2).map_or(i, |n| n as i64);
        let b = st.as_bytes();
        Ok((i..=j)
            .filter_map(|k| {
                b.get((k - 1).max(0) as usize)
                    .map(|c| Val::Num(f64::from(*c)))
            })
            .collect())
    });
    reg(&s, "char", |_, a| {
        let bytes: Vec<u8> = a.iter().map(|v| v.to_num().unwrap_or(0.0) as u8).collect();
        Ok(vec![Val::str(&String::from_utf8_lossy(&bytes))])
    });
    reg(&s, "find", |_, a| {
        let st = arg_str(&a, 0, "find")?;
        let p = arg_str(&a, 1, "find")?;
        let init = opt_num(&a, 2).unwrap_or(1.0) as i64;
        let plain = arg(&a, 3).truthy();
        Ok(match pat::find(&st, &p, init, plain).map_err(pat_err)? {
            Some(f) => {
                let mut v = vec![Val::Num(f.start as f64), Val::Num(f.end as f64)];
                v.extend(caps_to_vals(&f.captures));
                v
            }
            None => vec![Val::Nil],
        })
    });
    reg(&s, "match", |_, a| {
        let st = arg_str(&a, 0, "match")?;
        let p = arg_str(&a, 1, "match")?;
        let init = opt_num(&a, 2).unwrap_or(1.0) as i64;
        Ok(match pat::matches(&st, &p, init).map_err(pat_err)? {
            Some(c) => caps_to_vals(&c),
            None => vec![Val::Nil],
        })
    });
    reg(&s, "gmatch", |_, a| {
        let st = arg_str(&a, 0, "gmatch")?;
        let p = arg_str(&a, 1, "gmatch")?;
        let all: Vec<Vec<Val>> = pat::gmatch(&st, &p)
            .map_err(pat_err)?
            .iter()
            .map(|c| caps_to_vals(c))
            .collect();
        let pos = RefCell::new(0usize);
        Ok(vec![Val::native(move |_, _| {
            let mut i = pos.borrow_mut();
            let out = all.get(*i).cloned().unwrap_or_else(|| vec![Val::Nil]);
            *i += 1;
            Ok(out)
        })])
    });
    reg(&s, "gsub", |it, a| {
        let st = arg_str(&a, 0, "gsub")?;
        let p = arg_str(&a, 1, "gsub")?;
        let repl = arg(&a, 2);
        let n = opt_num(&a, 3).map(|n| n as usize);
        let (out, count) = match &repl {
            Val::Str(_) | Val::Num(_) => {
                let r = tostring(&repl);
                pat::gsub(&st, &p, pat::Repl::Str(&r), n).map_err(pat_err)?
            }
            Val::Table(t) => {
                let t = t.clone();
                let mut f = |caps: &[Capture<'_>]| -> Option<String> {
                    let key = caps_to_vals(&caps[..1]).remove(0);
                    let v = t.get(&key);
                    v.truthy().then(|| tostring(&v))
                };
                pat::gsub(&st, &p, pat::Repl::Func(&mut f), n).map_err(pat_err)?
            }
            Val::Func(_) => {
                let mut err: Option<LuaError> = None;
                let mut f = |caps: &[Capture<'_>]| -> Option<String> {
                    if err.is_some() {
                        return None;
                    }
                    match it.call(&repl, caps_to_vals(caps)) {
                        Ok(v) => {
                            let v = v.into_iter().next().unwrap_or_default();
                            v.truthy().then(|| tostring(&v))
                        }
                        Err(e) => {
                            err = Some(e);
                            None
                        }
                    }
                };
                let res = pat::gsub(&st, &p, pat::Repl::Func(&mut f), n).map_err(pat_err)?;
                if let Some(e) = err {
                    return Err(e);
                }
                res
            }
            other => {
                return Err(LuaError::msg(format!(
                    "bad argument #3 to 'gsub' (got {})",
                    other.type_name()
                )))
            }
        };
        Ok(vec![Val::str(&out), Val::Num(count as f64)])
    });
    reg(&s, "format", |_, a| Ok(vec![Val::str(&format_lua(&a)?)]));
    g.set_str("string", Val::Table(s));

    // ---- table ----
    let t = TableRef::new();
    reg(&t, "insert", |_, a| {
        let tb = arg_table(&a, 0, "insert")?;
        match a.len() {
            2 => tb.push(arg(&a, 1)),
            3 => {
                let pos = arg_num(&a, 1, "insert")? as usize;
                let mut inner = tb.0.borrow_mut();
                let len = inner.arr.len();
                if pos >= 1 && pos <= len + 1 {
                    inner.arr.insert(pos - 1, arg(&a, 2));
                } else {
                    drop(inner);
                    tb.set(Val::Num(pos as f64), arg(&a, 2))?;
                }
            }
            _ => return Err(LuaError::msg("wrong number of arguments to 'insert'")),
        }
        Ok(Vec::new())
    });
    reg(&t, "remove", |_, a| {
        let tb = arg_table(&a, 0, "remove")?;
        let mut inner = tb.0.borrow_mut();
        let len = inner.arr.len();
        if len == 0 {
            return Ok(vec![Val::Nil]);
        }
        let pos = opt_num(&a, 1).map_or(len, |n| n as usize);
        if pos < 1 || pos > len {
            return Ok(vec![Val::Nil]);
        }
        Ok(vec![inner.arr.remove(pos - 1)])
    });
    reg(&t, "concat", |_, a| {
        let tb = arg_table(&a, 0, "concat")?;
        let sep = if a.len() > 1 && !arg(&a, 1).is_nil() {
            arg_str(&a, 1, "concat")?
        } else {
            String::new()
        };
        let i = opt_num(&a, 2).unwrap_or(1.0) as i64;
        let j = opt_num(&a, 3).map_or(tb.len() as i64, |n| n as i64);
        let mut parts = Vec::new();
        for k in i..=j {
            match tb.get_int(k) {
                Val::Str(s) => parts.push(s.to_string()),
                Val::Num(n) => parts.push(num_to_string(n)),
                other => {
                    return Err(LuaError::msg(format!(
                        "invalid value (at index {k}) in table for 'concat' ({})",
                        other.type_name()
                    )))
                }
            }
        }
        Ok(vec![Val::str(&parts.join(&sep))])
    });
    reg(&t, "sort", |it, a| {
        let tb = arg_table(&a, 0, "sort")?;
        let cmp = arg(&a, 1);
        let mut items = tb.0.borrow().arr.clone();
        let mut err: Option<LuaError> = None;
        // Insertion-merge via sort_by; comparator errors are surfaced after.
        items.sort_by(|x, y| {
            if err.is_some() {
                return std::cmp::Ordering::Equal;
            }
            let less = |p: &Val, q: &Val, it: &mut Interp| -> R<bool> {
                if cmp.is_nil() {
                    lt(p, q)
                } else {
                    Ok(it
                        .call(&cmp, vec![p.clone(), q.clone()])?
                        .first()
                        .is_some_and(Val::truthy))
                }
            };
            match less(x, y, it) {
                Ok(true) => std::cmp::Ordering::Less,
                Ok(false) => match less(y, x, it) {
                    Ok(true) => std::cmp::Ordering::Greater,
                    Ok(false) => std::cmp::Ordering::Equal,
                    Err(e) => {
                        err = Some(e);
                        std::cmp::Ordering::Equal
                    }
                },
                Err(e) => {
                    err = Some(e);
                    std::cmp::Ordering::Equal
                }
            }
        });
        if let Some(e) = err {
            return Err(e);
        }
        tb.0.borrow_mut().arr = items;
        Ok(Vec::new())
    });
    t.set_str("unpack", g.get_str("unpack"));
    g.set_str("table", Val::Table(t));

    // ---- math ----
    let m = TableRef::new();
    reg(&m, "floor", |_, a| {
        Ok(vec![Val::Num(arg_num(&a, 0, "floor")?.floor())])
    });
    reg(&m, "ceil", |_, a| {
        Ok(vec![Val::Num(arg_num(&a, 0, "ceil")?.ceil())])
    });
    reg(&m, "abs", |_, a| {
        Ok(vec![Val::Num(arg_num(&a, 0, "abs")?.abs())])
    });
    reg(&m, "sqrt", |_, a| {
        Ok(vec![Val::Num(arg_num(&a, 0, "sqrt")?.sqrt())])
    });
    reg(&m, "exp", |_, a| {
        Ok(vec![Val::Num(arg_num(&a, 0, "exp")?.exp())])
    });
    reg(&m, "log", |_, a| {
        let x = arg_num(&a, 0, "log")?;
        Ok(vec![Val::Num(match opt_num(&a, 1) {
            Some(b) => x.ln() / b.ln(),
            None => x.ln(),
        })])
    });
    reg(&m, "pow", |_, a| {
        Ok(vec![Val::Num(
            arg_num(&a, 0, "pow")?.powf(arg_num(&a, 1, "pow")?),
        )])
    });
    reg(&m, "fmod", |_, a| {
        Ok(vec![Val::Num(
            arg_num(&a, 0, "fmod")? % arg_num(&a, 1, "fmod")?,
        )])
    });
    reg(&m, "modf", |_, a| {
        let x = arg_num(&a, 0, "modf")?;
        let i = x.trunc();
        Ok(vec![Val::Num(i), Val::Num(x - i)])
    });
    reg(&m, "min", |_, a| {
        let mut r = arg_num(&a, 0, "min")?;
        for i in 1..a.len() {
            r = r.min(arg_num(&a, i, "min")?);
        }
        Ok(vec![Val::Num(r)])
    });
    reg(&m, "max", |_, a| {
        let mut r = arg_num(&a, 0, "max")?;
        for i in 1..a.len() {
            r = r.max(arg_num(&a, i, "max")?);
        }
        Ok(vec![Val::Num(r)])
    });
    m.set_str("huge", Val::Num(f64::INFINITY));
    m.set_str("pi", Val::Num(std::f64::consts::PI));
    g.set_str("math", Val::Table(m));

    // ---- bit (LuaJIT semantics: 32-bit signed results) ----
    let b = TableRef::new();
    let to_i32 = |v: f64| -> i32 { (v as i64 as u64 & 0xFFFF_FFFF) as u32 as i32 };
    reg(&b, "band", move |_, a| {
        let mut r: i32 = -1;
        for i in 0..a.len() {
            r &= to_i32(arg_num(&a, i, "band")?);
        }
        Ok(vec![Val::Num(f64::from(r))])
    });
    reg(&b, "bor", move |_, a| {
        let mut r: i32 = 0;
        for i in 0..a.len() {
            r |= to_i32(arg_num(&a, i, "bor")?);
        }
        Ok(vec![Val::Num(f64::from(r))])
    });
    reg(&b, "bxor", move |_, a| {
        let mut r: i32 = 0;
        for i in 0..a.len() {
            r ^= to_i32(arg_num(&a, i, "bxor")?);
        }
        Ok(vec![Val::Num(f64::from(r))])
    });
    reg(&b, "bnot", move |_, a| {
        Ok(vec![Val::Num(f64::from(!to_i32(arg_num(&a, 0, "bnot")?)))])
    });
    reg(&b, "lshift", move |_, a| {
        let x = to_i32(arg_num(&a, 0, "lshift")?);
        let n = arg_num(&a, 1, "lshift")? as u32 & 31;
        Ok(vec![Val::Num(f64::from(x.wrapping_shl(n)))])
    });
    reg(&b, "rshift", move |_, a| {
        let x = to_i32(arg_num(&a, 0, "rshift")?) as u32;
        let n = arg_num(&a, 1, "rshift")? as u32 & 31;
        Ok(vec![Val::Num(f64::from((x >> n) as i32))])
    });
    g.set_str("bit", Val::Table(b));
    // ---- os (minimal) ----
    let os = TableRef::new();
    let start = std::time::Instant::now();
    reg(&os, "clock", move |_, _| {
        Ok(vec![Val::Num(start.elapsed().as_secs_f64())])
    });
    reg(&os, "time", |_, _| {
        #[cfg(not(target_arch = "wasm32"))]
        let t = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0.0, |d| d.as_secs_f64().floor());
        #[cfg(target_arch = "wasm32")]
        let t = 0.0;
        Ok(vec![Val::Num(t)])
    });
    reg(&os, "getenv", |_, _| Ok(vec![Val::Nil]));
    g.set_str("os", Val::Table(os));
    g.set_str("_G", Val::Table(g.clone()));
}

/// `string.format` for the specifiers PoB uses.
fn format_lua(a: &[Val]) -> R<String> {
    let fmt = arg_str(a, 0, "format")?;
    let bytes = fmt.as_bytes();
    let mut out = String::new();
    let mut argi = 1;
    let mut i = 0;
    while i < bytes.len() {
        let c = bytes[i];
        if c != b'%' {
            // Copy the full UTF-8 char.
            let ch_len = fmt[i..].chars().next().map_or(1, char::len_utf8);
            out.push_str(&fmt[i..i + ch_len]);
            i += ch_len;
            continue;
        }
        i += 1;
        if i < bytes.len() && bytes[i] == b'%' {
            out.push('%');
            i += 1;
            continue;
        }
        let start = i;
        while i < bytes.len() && b"-+ #0123456789.".contains(&bytes[i]) {
            i += 1;
        }
        let spec = &fmt[start..i];
        let conv = *bytes
            .get(i)
            .ok_or_else(|| LuaError::msg("invalid format string"))?;
        i += 1;
        let (flags, width, prec) = parse_spec(spec);
        let v = arg(a, argi);
        argi += 1;
        let body = match conv {
            b'd' | b'i' => {
                let n = v
                    .to_num()
                    .ok_or_else(|| LuaError::msg("bad argument to 'format' (number expected)"))?;
                let n = n.trunc() as i64;
                let s = n.abs().to_string();
                let sign = if n < 0 {
                    "-"
                } else if flags.contains('+') {
                    "+"
                } else {
                    ""
                };
                format!("{sign}{s}")
            }
            b'x' | b'X' => {
                let n = v.to_num().unwrap_or(0.0) as i64;
                if conv == b'x' {
                    format!("{n:x}")
                } else {
                    format!("{n:X}")
                }
            }
            b'f' | b'F' => {
                let n = v
                    .to_num()
                    .ok_or_else(|| LuaError::msg("bad argument to 'format' (number expected)"))?;
                let p = prec.unwrap_or(6);
                let s = format!("{:.*}", p, n.abs());
                let sign = if n < 0.0
                    || (n == 0.0
                        && n.is_sign_negative()
                        && s.bytes().any(|b| b != b'0' && b != b'.'))
                {
                    "-"
                } else if flags.contains('+') {
                    "+"
                } else {
                    ""
                };
                format!("{sign}{s}")
            }
            b'g' | b'G' => {
                let n = v.to_num().unwrap_or(0.0);
                if prec.is_none() {
                    num_to_string(n)
                } else {
                    format!("{:.*}", prec.unwrap_or(6), n)
                }
            }
            b'e' | b'E' => format!("{:.*e}", prec.unwrap_or(6), v.to_num().unwrap_or(0.0)),
            b's' => {
                let s = tostring(&v);
                match prec {
                    Some(p) => s.chars().take(p).collect(),
                    None => s,
                }
            }
            b'q' => format!("{:?}", tostring(&v)),
            b'c' => char::from(v.to_num().unwrap_or(0.0) as u8).to_string(),
            other => {
                return Err(LuaError::msg(format!(
                    "invalid option '%{}' to 'format'",
                    other as char
                )))
            }
        };
        let pad = width.saturating_sub(body.chars().count());
        if flags.contains('-') {
            out.push_str(&body);
            out.push_str(&" ".repeat(pad));
        } else if flags.contains('0') && matches!(conv, b'd' | b'i' | b'f' | b'F' | b'x' | b'X') {
            let (sign, digits) = if body.starts_with(['-', '+']) {
                body.split_at(1)
            } else {
                ("", body.as_str())
            };
            out.push_str(sign);
            out.push_str(&"0".repeat(pad));
            out.push_str(digits);
        } else {
            out.push_str(&" ".repeat(pad));
            out.push_str(&body);
        }
    }
    Ok(out)
}

fn parse_spec(spec: &str) -> (String, usize, Option<usize>) {
    let flags: String = spec.chars().take_while(|c| "-+ #0".contains(*c)).collect();
    let rest = &spec[flags.len()..];
    let (w, p) = match rest.split_once('.') {
        Some((w, p)) => (w, Some(p.parse().unwrap_or(0))),
        None => (rest, None),
    };
    (flags, w.parse().unwrap_or(0), p)
}
