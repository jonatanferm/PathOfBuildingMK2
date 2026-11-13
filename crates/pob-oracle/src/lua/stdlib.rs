//! The subset of Lua 5.1's standard library (+ LuaJIT `bit`) PoB code uses.

use std::cell::RefCell;

use super::interp::{lt, Interp, LuaError};
use super::value::{num_to_string, LStr, TableRef, Val};
use pob_lua::pattern::{self as pat, Capture};

type R<T> = Result<T, LuaError>;

pub(crate) fn arg(args: &[Val], i: usize) -> Val {
    args.get(i).cloned().unwrap_or_default()
}

pub(crate) fn arg_str(args: &[Val], i: usize, fname: &str) -> R<String> {
    match args.get(i) {
        Some(Val::Str(s)) => Ok(s.to_str().into_owned()),
        Some(Val::Num(n)) => Ok(num_to_string(*n)),
        other => Err(LuaError::msg(format!(
            "bad argument #{} to '{fname}' (string expected, got {})",
            i + 1,
            other.map_or("no value", Val::type_name)
        ))),
    }
}

pub(crate) fn arg_num(args: &[Val], i: usize, fname: &str) -> R<f64> {
    args.get(i).and_then(Val::to_num).ok_or_else(|| {
        LuaError::msg(format!(
            "bad argument #{} to '{fname}' (number expected, got {})",
            i + 1,
            args.get(i).map_or("no value", Val::type_name)
        ))
    })
}

pub(crate) fn opt_num(args: &[Val], i: usize) -> Option<f64> {
    args.get(i).and_then(Val::to_num)
}

pub(crate) fn arg_table(args: &[Val], i: usize, fname: &str) -> R<TableRef> {
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
            Capture::Str(s) => Val::bytes(s),
            Capture::Pos(p) => Val::Num(*p as f64),
        })
        .collect()
}

/// A string argument as bytes (numbers are converted, like Lua).
pub(crate) fn arg_lstr(args: &[Val], i: usize, fname: &str) -> R<LStr> {
    match args.get(i) {
        Some(Val::Str(s)) => Ok(s.clone()),
        Some(Val::Num(n)) => Ok(LStr::new(&num_to_string(*n))),
        other => Err(LuaError::msg(format!(
            "bad argument #{} to '{fname}' (string expected, got {})",
            i + 1,
            other.map_or("no value", Val::type_name)
        ))),
    }
}

/// Run a `pob_lua::pattern` call, turning its `lua_error!` (a `pob_lua::LuaError`
/// panic: malformed pattern, "pattern too complex") into the interpreter's error.
fn pat_call<T>(f: impl FnOnce() -> T) -> R<T> {
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(f)).map_err(|p| {
        match p.downcast::<pob_lua::LuaError>() {
            Ok(e) => LuaError::msg(e.0),
            Err(other) => std::panic::resume_unwind(other),
        }
    })
}

/// Lua `tostring`.
#[must_use]
pub fn tostring(v: &Val) -> String {
    v.to_string()
}

pub(crate) fn reg(
    t: &TableRef,
    name: &str,
    f: impl Fn(&mut Interp, Vec<Val>) -> R<Vec<Val>> + 'static,
) {
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
        Ok(vec![match arg(&a, 0) {
            s @ Val::Str(_) => s,
            other => Val::str(&tostring(&other)),
        }])
    });
    reg(&g, "tonumber", |_, a| {
        let v = arg(&a, 0);
        let base = opt_num(&a, 1);
        Ok(vec![match (&v, base) {
            (Val::Num(n), None) => Val::Num(*n),
            (Val::Str(s), None) => pob_lua::tonumber(s.as_str()).map_or(Val::Nil, Val::Num),
            (Val::Str(s), Some(b)) => i64::from_str_radix(s.as_str().trim(), b as u32)
                .map_or(Val::Nil, |n| Val::Num(n as f64)),
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
    reg(&g, "setmetatable", |_, a| {
        let t = arg_table(&a, 0, "setmetatable")?;
        t.0.borrow_mut().meta = match arg(&a, 1) {
            Val::Table(m) => Some(m),
            _ => None,
        };
        Ok(vec![Val::Table(t)])
    });
    reg(&g, "getmetatable", |it, a| {
        Ok(vec![match arg(&a, 0) {
            Val::Table(t) => t.meta().map_or(Val::Nil, Val::Table),
            Val::Str(_) => {
                let m = TableRef::new();
                m.set_str("__index", Val::Table(it.string_lib.clone()));
                Val::Table(m)
            }
            _ => Val::Nil,
        }])
    });
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

    // ---- string (byte semantics) ----
    let s = it.string_lib.clone();
    reg(&s, "lower", |_, a| {
        Ok(vec![Val::bytes(
            &arg_lstr(&a, 0, "lower")?.as_bytes().to_ascii_lowercase(),
        )])
    });
    reg(&s, "upper", |_, a| {
        Ok(vec![Val::bytes(
            &arg_lstr(&a, 0, "upper")?.as_bytes().to_ascii_uppercase(),
        )])
    });
    reg(&s, "len", |_, a| {
        Ok(vec![Val::Num(arg_lstr(&a, 0, "len")?.len() as f64)])
    });
    reg(&s, "rep", |_, a| {
        let st = arg_lstr(&a, 0, "rep")?;
        let n = arg_num(&a, 1, "rep")?.max(0.0) as usize;
        Ok(vec![Val::bytes(&st.as_bytes().repeat(n))])
    });
    reg(&s, "reverse", |_, a| {
        let mut b = arg_lstr(&a, 0, "reverse")?.as_bytes().to_vec();
        b.reverse();
        Ok(vec![Val::bytes(&b)])
    });
    reg(&s, "sub", |_, a| {
        let st = arg_lstr(&a, 0, "sub")?;
        let b = st.as_bytes();
        let l = b.len() as i64;
        let norm = |i: i64| if i < 0 { l + i + 1 } else { i };
        let mut i = norm(opt_num(&a, 1).unwrap_or(1.0) as i64);
        let mut j = norm(opt_num(&a, 2).unwrap_or(-1.0) as i64);
        if i < 1 {
            i = 1;
        }
        if j > l {
            j = l;
        }
        Ok(vec![if i > j {
            Val::str("")
        } else {
            Val::bytes(&b[(i - 1) as usize..j as usize])
        }])
    });
    reg(&s, "byte", |_, a| {
        let st = arg_lstr(&a, 0, "byte")?;
        let b = st.as_bytes();
        let l = b.len() as i64;
        let norm = |i: i64| if i < 0 { l + i + 1 } else { i };
        let i = norm(opt_num(&a, 1).unwrap_or(1.0) as i64);
        let j = norm(opt_num(&a, 2).map_or(i, |n| n as i64));
        Ok((i.max(1)..=j.min(l))
            .map(|k| Val::Num(f64::from(b[(k - 1) as usize])))
            .collect())
    });
    reg(&s, "char", |_, a| {
        let bytes: Vec<u8> = a.iter().map(|v| v.to_num().unwrap_or(0.0) as u8).collect();
        Ok(vec![Val::bytes(&bytes)])
    });
    reg(&s, "find", |_, a| {
        let st = arg_lstr(&a, 0, "find")?;
        let p = arg_lstr(&a, 1, "find")?;
        let init = opt_num(&a, 2).unwrap_or(1.0) as i64;
        let plain = arg(&a, 3).truthy();
        Ok(
            match pat_call(|| {
                pat::find_bytes(st.as_bytes(), p.as_bytes(), Some(init), plain)
                    .map(|(s, e, c)| (s, e, caps_to_vals(&c)))
            })? {
                Some((start, end, captures)) => {
                    let mut v = vec![Val::Num(start as f64), Val::Num(end as f64)];
                    v.extend(captures);
                    v
                }
                None => vec![Val::Nil],
            },
        )
    });
    reg(&s, "match", |_, a| {
        let st = arg_lstr(&a, 0, "match")?;
        let p = arg_lstr(&a, 1, "match")?;
        let init = opt_num(&a, 2).unwrap_or(1.0) as i64;
        Ok(
            match pat_call(|| {
                pat::match_bytes(st.as_bytes(), p.as_bytes(), Some(init)).map(|c| caps_to_vals(&c))
            })? {
                Some(c) => c,
                None => vec![Val::Nil],
            },
        )
    });
    reg(&s, "gmatch", |_, a| {
        let st = arg_lstr(&a, 0, "gmatch")?;
        let p = arg_lstr(&a, 1, "gmatch")?;
        let all: Vec<Vec<Val>> = pat_call(|| {
            pat::gmatch_bytes(st.as_bytes(), p.as_bytes())
                .map(|c| caps_to_vals(&c))
                .collect()
        })?;
        let pos = RefCell::new(0usize);
        Ok(vec![Val::native(move |_, _| {
            let mut i = pos.borrow_mut();
            let out = all.get(*i).cloned().unwrap_or_else(|| vec![Val::Nil]);
            *i += 1;
            Ok(out)
        })])
    });
    reg(&s, "gsub", |it, a| {
        let st = arg_lstr(&a, 0, "gsub")?;
        let p = arg_lstr(&a, 1, "gsub")?;
        let repl = arg(&a, 2);
        let n = opt_num(&a, 3).map(|n| n as i64);
        let to_bytes = |v: &Val| -> Vec<u8> {
            match v {
                Val::Str(s) => s.as_bytes().to_vec(),
                other => tostring(other).into_bytes(),
            }
        };
        let (out, count) = match &repl {
            Val::Str(_) | Val::Num(_) => {
                let r = to_bytes(&repl);
                pat_call(|| {
                    pat::gsub_bytes_with(st.as_bytes(), p.as_bytes(), pat::Repl::Str(&r), n)
                })?
            }
            Val::Table(t) => {
                let t = t.clone();
                let mut f = |cap: &Capture<'_>| -> Option<Vec<u8>> {
                    let key = caps_to_vals(std::slice::from_ref(cap)).remove(0);
                    let v = t.get(&key);
                    v.truthy().then(|| to_bytes(&v))
                };
                pat_call(|| {
                    pat::gsub_bytes_with(st.as_bytes(), p.as_bytes(), pat::Repl::Table(&mut f), n)
                })?
            }
            Val::Func(_) => {
                let mut err: Option<LuaError> = None;
                let mut f = |caps: &pat::Captures<'_>| -> Option<Vec<u8>> {
                    if err.is_some() {
                        return None;
                    }
                    match it.call(&repl, caps_to_vals(caps)) {
                        Ok(v) => {
                            let v = v.into_iter().next().unwrap_or_default();
                            v.truthy().then(|| to_bytes(&v))
                        }
                        Err(e) => {
                            err = Some(e);
                            None
                        }
                    }
                };
                let res = pat_call(|| {
                    pat::gsub_bytes_with(st.as_bytes(), p.as_bytes(), pat::Repl::Func(&mut f), n)
                })?;
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
        Ok(vec![Val::bytes(&out), Val::Num(count as f64)])
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
        let sep: Vec<u8> = if a.len() > 1 && !arg(&a, 1).is_nil() {
            arg_lstr(&a, 1, "concat")?.as_bytes().to_vec()
        } else {
            Vec::new()
        };
        let i = opt_num(&a, 2).unwrap_or(1.0) as i64;
        let j = opt_num(&a, 3).map_or(tb.len() as i64, |n| n as i64);
        let mut out: Vec<u8> = Vec::new();
        for k in i..=j {
            if k > i {
                out.extend_from_slice(&sep);
            }
            match tb.get_int(k) {
                Val::Str(s) => out.extend_from_slice(s.as_bytes()),
                Val::Num(n) => out.extend_from_slice(num_to_string(n).as_bytes()),
                other => {
                    return Err(LuaError::msg(format!(
                        "invalid value (at index {k}) in table for 'concat' ({})",
                        other.type_name()
                    )))
                }
            }
        }
        Ok(vec![Val::bytes(&out)])
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
    for (name, f) in [
        ("sin", f64::sin as fn(f64) -> f64),
        ("cos", f64::cos),
        ("tan", f64::tan),
        ("asin", f64::asin),
        ("acos", f64::acos),
        ("sinh", f64::sinh),
        ("cosh", f64::cosh),
        ("tanh", f64::tanh),
        ("deg", f64::to_degrees),
        ("rad", f64::to_radians),
        ("log10", f64::log10),
    ] {
        reg(&m, name, move |_, a| {
            Ok(vec![Val::Num(f(arg_num(&a, 0, name)?))])
        });
    }
    reg(&m, "atan", |_, a| {
        let y = arg_num(&a, 0, "atan")?;
        Ok(vec![Val::Num(match opt_num(&a, 1) {
            Some(x) => y.atan2(x),
            None => y.atan(),
        })])
    });
    reg(&m, "atan2", |_, a| {
        Ok(vec![Val::Num(
            arg_num(&a, 0, "atan2")?.atan2(arg_num(&a, 1, "atan2")?),
        )])
    });
    reg(&m, "frexp", |_, a| {
        let x = arg_num(&a, 0, "frexp")?;
        if x == 0.0 || !x.is_finite() {
            return Ok(vec![Val::Num(x), Val::Num(0.0)]);
        }
        let e = x.abs().log2().floor() + 1.0;
        Ok(vec![Val::Num(x / 2f64.powf(e)), Val::Num(e)])
    });
    reg(&m, "ldexp", |_, a| {
        Ok(vec![Val::Num(
            arg_num(&a, 0, "ldexp")? * 2f64.powf(arg_num(&a, 1, "ldexp")?),
        )])
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
    reg(&b, "arshift", move |_, a| {
        let x = to_i32(arg_num(&a, 0, "arshift")?);
        let n = arg_num(&a, 1, "arshift")? as u32 & 31;
        Ok(vec![Val::Num(f64::from(x >> n))])
    });
    reg(&b, "rol", move |_, a| {
        let x = to_i32(arg_num(&a, 0, "rol")?) as u32;
        let n = arg_num(&a, 1, "rol")? as u32 & 31;
        Ok(vec![Val::Num(f64::from(x.rotate_left(n) as i32))])
    });
    reg(&b, "ror", move |_, a| {
        let x = to_i32(arg_num(&a, 0, "ror")?) as u32;
        let n = arg_num(&a, 1, "ror")? as u32 & 31;
        Ok(vec![Val::Num(f64::from(x.rotate_right(n) as i32))])
    });
    reg(&b, "tobit", move |_, a| {
        Ok(vec![Val::Num(f64::from(to_i32(arg_num(&a, 0, "tobit")?)))])
    });
    reg(&b, "bswap", move |_, a| {
        Ok(vec![Val::Num(f64::from(
            to_i32(arg_num(&a, 0, "bswap")?).swap_bytes(),
        ))])
    });
    reg(&b, "tohex", move |_, a| {
        let x = to_i32(arg_num(&a, 0, "tohex")?) as u32;
        let n = opt_num(&a, 1).unwrap_or(8.0) as i32;
        let digits = n.unsigned_abs().min(8) as usize;
        let hex = if n < 0 {
            format!("{x:08X}")
        } else {
            format!("{x:08x}")
        };
        Ok(vec![Val::str(&hex[8 - digits..])])
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
                let s = fmt_g(n, prec.unwrap_or(6), flags.contains('#'));
                if conv == b'G' {
                    s.to_uppercase()
                } else {
                    s
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

/// C `%.{prec}g`.
fn fmt_g(n: f64, prec: usize, alt: bool) -> String {
    if n.is_nan() {
        return "nan".into();
    }
    if n.is_infinite() {
        return if n > 0.0 { "inf".into() } else { "-inf".into() };
    }
    let p = prec.max(1);
    if n == 0.0 {
        return if alt {
            format!("{:.*}", p - 1, 0.0)
        } else {
            "0".into()
        };
    }
    // Exponent after rounding to `p` significant digits.
    let sci = format!("{:.*e}", p - 1, n);
    let (mant, exp) = sci.split_once('e').unwrap_or((&sci, "0"));
    let exp: i32 = exp.parse().unwrap_or(0);
    let trim = |mut s: String| {
        if !alt && s.contains('.') {
            while s.ends_with('0') {
                s.pop();
            }
            if s.ends_with('.') {
                s.pop();
            }
        }
        s
    };
    if exp < -4 || exp >= p as i32 {
        let m = trim(mant.to_owned());
        format!("{m}e{}{:02}", if exp < 0 { '-' } else { '+' }, exp.abs())
    } else {
        let decimals = (p as i32 - 1 - exp).max(0) as usize;
        trim(format!("{:.*}", decimals, n))
    }
}
