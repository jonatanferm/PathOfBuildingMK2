//! Tree-walking evaluator for the slot-resolved AST.

use std::cell::RefCell;
use std::fmt;
use std::rc::Rc;

use super::parser::{BinOp, Block, Expr, FuncProto, Parser, Stat, TField, UnOp, UpvalDesc, VarRef};
use super::value::{num_to_string, Func, LStr, TableRef, Val};

#[derive(Debug, Clone)]
pub struct LuaError(pub String);

impl LuaError {
    #[must_use]
    pub fn msg(m: impl Into<String>) -> Self {
        Self(m.into())
    }
}

impl fmt::Display for LuaError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for LuaError {}

type Cell = Rc<RefCell<Val>>;
type R<T> = Result<T, LuaError>;

pub struct Closure {
    pub proto: Rc<FuncProto>,
    pub upvals: Vec<Cell>,
}

struct Frame<'c> {
    /// Uncaptured locals.
    regs: Vec<Val>,
    /// Captured locals (shared with closures); `None` until declared.
    cells: Vec<Option<Cell>>,
    captured: &'c [bool],
    varargs: Vec<Val>,
    closure: &'c Closure,
}

enum Flow {
    Normal,
    Break,
    Return(Vec<Val>),
    Goto(LStr),
}

/// The interpreter: globals plus call-depth bookkeeping.
pub struct Interp {
    pub globals: TableRef,
    k_index: LStr,
    k_newindex: LStr,
    k_call: LStr,
    /// `string` library table, used for `s:method()` lookups.
    pub string_lib: TableRef,
    depth: u32,
}

impl Default for Interp {
    fn default() -> Self {
        Self::new()
    }
}

impl Interp {
    #[must_use]
    pub fn new() -> Self {
        let mut it = Self {
            globals: TableRef::new(),
            k_index: LStr::intern("__index"),
            k_newindex: LStr::intern("__newindex"),
            k_call: LStr::intern("__call"),
            string_lib: TableRef::new(),
            depth: 0,
        };
        super::stdlib::install(&mut it);
        it
    }

    pub fn set_global(&self, name: &str, v: Val) {
        self.globals.set_str(name, v);
    }

    #[must_use]
    pub fn global(&self, name: &str) -> Val {
        self.globals.get_str(name)
    }

    /// Compile a chunk into a function value (`load`).
    pub fn load_chunk(&mut self, src: &str, name: &str) -> R<Val> {
        let proto = Parser::new(src, name)?.parse_chunk()?;
        Ok(Val::Func(Func::Lua(Rc::new(Closure {
            proto,
            upvals: Vec::new(),
        }))))
    }

    /// Parse and run a chunk, returning its results.
    pub fn exec_chunk(&mut self, src: &str, name: &str, args: Vec<Val>) -> R<Vec<Val>> {
        let f = self.load_chunk(src, name)?;
        self.call(&f, args)
    }

    /// Call any function value.
    pub fn call(&mut self, f: &Val, args: Vec<Val>) -> R<Vec<Val>> {
        match f {
            Val::Func(Func::Native(nf)) => {
                let nf = nf.clone();
                nf(self, args)
            }
            Val::Func(Func::Lua(c)) => {
                let c = c.clone();
                self.call_closure(&c, args)
            }
            Val::Table(t) => {
                let handler = t
                    .meta()
                    .map(|m| m.get_lstr(&self.k_call))
                    .unwrap_or_default();
                if handler.is_nil() {
                    return Err(LuaError::msg("attempt to call a table value"));
                }
                let mut full = Vec::with_capacity(args.len() + 1);
                full.push(f.clone());
                full.extend(args);
                self.call(&handler, full)
            }
            other => Err(LuaError::msg(format!(
                "attempt to call a {} value",
                other.type_name()
            ))),
        }
    }

    fn call_closure(&mut self, c: &Closure, mut args: Vec<Val>) -> R<Vec<Val>> {
        self.depth += 1;
        if self.depth > 190 {
            self.depth -= 1;
            return Err(LuaError::msg("stack overflow"));
        }
        let proto = &c.proto;
        let n = proto.nslots as usize;
        let any_captured = proto.captured.iter().any(|c| *c);
        let mut frame = Frame {
            regs: vec![Val::Nil; n],
            cells: if any_captured {
                vec![None; n]
            } else {
                Vec::new()
            },
            captured: &proto.captured,
            varargs: Vec::new(),
            closure: c,
        };
        let nparams = proto.params.len();
        if proto.is_vararg && args.len() > nparams {
            frame.varargs = args.split_off(nparams);
        }
        args.resize(nparams, Val::Nil);
        for (slot, v) in proto.params.iter().zip(args) {
            Self::set_local(&mut frame, *slot, v);
        }
        let res = self.exec_block(&proto.body, &mut frame).map_err(|mut e| {
            // Lua-style traceback, capped so deep recursion stays readable.
            if e.0.matches("\n\tin ").count() < 12 {
                e.0.push_str(&format!(
                    "\n\tin {} ({}:{})",
                    proto.name, proto.source, proto.line
                ));
            }
            e
        });
        self.depth -= 1;
        match res? {
            Flow::Return(v) => Ok(v),
            _ => Ok(Vec::new()),
        }
    }

    fn exec_block(&mut self, block: &Block, fr: &mut Frame<'_>) -> R<Flow> {
        let mut i = 0;
        while i < block.len() {
            let flow = self.exec_stat(&block[i], fr).map_err(|mut e| {
                // Innermost statement: prefix `source:line:` like Lua does.
                let src = &fr.closure.proto.source;
                if !e.0.starts_with(&**src) && !e.0.contains(" [at ") {
                    e.0 = format!(
                        "{src}:{}: {}",
                        block.lines.get(i).copied().unwrap_or(0),
                        e.0
                    );
                }
                e
            })?;
            match flow {
                Flow::Normal => i += 1,
                Flow::Goto(label) => {
                    match block
                        .iter()
                        .position(|s| matches!(s, Stat::Label(l) if *l == label))
                    {
                        Some(p) => i = p + 1,
                        None => return Ok(Flow::Goto(label)),
                    }
                }
                other => return Ok(other),
            }
        }
        Ok(Flow::Normal)
    }

    /// Declare (or re-declare) a local: captured slots get a fresh cell so
    /// closures from earlier iterations keep their own.
    fn set_local(fr: &mut Frame<'_>, slot: u32, v: Val) {
        let s = slot as usize;
        if fr.captured[s] {
            fr.cells[s] = Some(Rc::new(RefCell::new(v)));
        } else {
            fr.regs[s] = v;
        }
    }

    #[allow(clippy::too_many_lines)]
    fn exec_stat(&mut self, stat: &Stat, fr: &mut Frame<'_>) -> R<Flow> {
        match stat {
            Stat::Local(slots, exprs)
                if slots.len() == 1
                    && exprs.len() == 1
                    && !matches!(exprs[0], Expr::Call(..) | Expr::Method(..) | Expr::Vararg) =>
            {
                let v = self.eval(&exprs[0], fr)?;
                Self::set_local(fr, slots[0], v);
            }
            Stat::Local(slots, exprs) => {
                let vals = self.eval_list(exprs, fr, slots.len())?;
                for (slot, v) in slots.iter().zip(vals) {
                    Self::set_local(fr, *slot, v);
                }
            }
            Stat::LocalFunc(slot, proto) => {
                // Declare first so the function can see itself.
                Self::set_local(fr, *slot, Val::Nil);
                let f = self.make_closure(proto, fr);
                let s = *slot as usize;
                if fr.captured[s] {
                    *fr.cells[s].as_ref().expect("cell").borrow_mut() = f;
                } else {
                    fr.regs[s] = f;
                }
            }
            Stat::Assign(targets, exprs) if targets.len() == 1 && exprs.len() == 1 => {
                match &targets[0] {
                    Expr::Var(var) => {
                        let v = self.eval(&exprs[0], fr)?;
                        self.assign_var(var, v, fr);
                    }
                    Expr::Index(obj, key) => {
                        let o = self.eval(obj, fr)?;
                        let k = self.eval(key, fr)?;
                        let v = self.eval(&exprs[0], fr)?;
                        self.set_index(&o, k, v)?;
                    }
                    _ => return Err(LuaError::msg("invalid assignment target")),
                }
            }
            Stat::Assign(targets, exprs) => {
                // Evaluate table/key operands of targets first, then values.
                let mut places = Vec::with_capacity(targets.len());
                for t in targets {
                    places.push(match t {
                        Expr::Index(obj, key) => {
                            let o = self.eval(obj, fr)?;
                            let k = self.eval(key, fr)?;
                            Place::Index(o, k)
                        }
                        Expr::Var(v) => Place::Var(v.clone()),
                        _ => return Err(LuaError::msg("invalid assignment target")),
                    });
                }
                let vals = self.eval_list(exprs, fr, places.len())?;
                for (place, v) in places.into_iter().zip(vals) {
                    match place {
                        Place::Var(var) => self.assign_var(&var, v, fr),
                        Place::Index(o, k) => self.set_index(&o, k, v)?,
                    }
                }
            }
            Stat::Call(e) => {
                self.eval_multi(e, fr)?;
            }
            Stat::Do(b) => return self.exec_block(b, fr),
            Stat::While(cond, body) => {
                while self.eval(cond, fr)?.truthy() {
                    match self.exec_block(body, fr)? {
                        Flow::Break => break,
                        Flow::Return(v) => return Ok(Flow::Return(v)),
                        Flow::Goto(l) => return Ok(Flow::Goto(l)),
                        Flow::Normal => {}
                    }
                }
            }
            Stat::Repeat(body, cond) => loop {
                match self.exec_block(body, fr)? {
                    Flow::Break => break,
                    Flow::Return(v) => return Ok(Flow::Return(v)),
                    Flow::Goto(l) => return Ok(Flow::Goto(l)),
                    Flow::Normal => {}
                }
                if self.eval(cond, fr)?.truthy() {
                    break;
                }
            },
            Stat::If(arms, else_block) => {
                for (cond, block) in arms {
                    if self.eval(cond, fr)?.truthy() {
                        return self.exec_block(block, fr);
                    }
                }
                if let Some(b) = else_block {
                    return self.exec_block(b, fr);
                }
            }
            Stat::NumFor {
                var,
                start,
                limit,
                step,
                body,
            } => {
                let start = self
                    .eval(start, fr)?
                    .to_num()
                    .ok_or_else(|| LuaError::msg("'for' initial value must be a number"))?;
                let limit = self
                    .eval(limit, fr)?
                    .to_num()
                    .ok_or_else(|| LuaError::msg("'for' limit must be a number"))?;
                let step = match step {
                    Some(s) => self
                        .eval(s, fr)?
                        .to_num()
                        .ok_or_else(|| LuaError::msg("'for' step must be a number"))?,
                    None => 1.0,
                };
                let mut i = start;
                while (step > 0.0 && i <= limit) || (step <= 0.0 && i >= limit) {
                    Self::set_local(fr, *var, Val::Num(i));
                    match self.exec_block(body, fr)? {
                        Flow::Break => break,
                        Flow::Return(v) => return Ok(Flow::Return(v)),
                        Flow::Goto(l) => return Ok(Flow::Goto(l)),
                        Flow::Normal => {}
                    }
                    i += step;
                }
            }
            Stat::GenFor { vars, exprs, body } => {
                let init = self.eval_list(exprs, fr, 3)?;
                let (f, s, mut ctl) = (init[0].clone(), init[1].clone(), init[2].clone());
                loop {
                    let mut rets = self.call(&f, vec![s.clone(), ctl.clone()])?;
                    let first = rets.first().cloned().unwrap_or_default();
                    if first.is_nil() {
                        break;
                    }
                    ctl = first;
                    rets.resize(vars.len(), Val::Nil);
                    for (slot, v) in vars.iter().zip(rets) {
                        Self::set_local(fr, *slot, v);
                    }
                    match self.exec_block(body, fr)? {
                        Flow::Break => break,
                        Flow::Return(v) => return Ok(Flow::Return(v)),
                        Flow::Goto(l) => return Ok(Flow::Goto(l)),
                        Flow::Normal => {}
                    }
                }
            }
            Stat::Return(exprs) => {
                // Proper tail position: all values of the last expression.
                let vals = self.eval_all(exprs, fr)?;
                return Ok(Flow::Return(vals));
            }
            Stat::Break => return Ok(Flow::Break),
            Stat::Goto(l) => return Ok(Flow::Goto(l.clone())),
            Stat::Label(_) => {}
        }
        Ok(Flow::Normal)
    }

    fn make_closure(&self, proto: &Rc<FuncProto>, fr: &Frame<'_>) -> Val {
        let upvals = proto
            .upvals
            .iter()
            .map(|d| match d {
                UpvalDesc::ParentLocal(s) => fr
                    .cells
                    .get(*s as usize)
                    .cloned()
                    .flatten()
                    .unwrap_or_else(|| Rc::new(RefCell::new(Val::Nil))),
                UpvalDesc::ParentUpval(i) => fr.closure.upvals[*i as usize].clone(),
            })
            .collect();
        Val::Func(Func::Lua(Rc::new(Closure {
            proto: proto.clone(),
            upvals,
        })))
    }

    fn read_var(&self, v: &VarRef, fr: &Frame<'_>) -> Val {
        match v {
            VarRef::Local(s) => {
                let s = *s as usize;
                if fr.captured[s] {
                    fr.cells[s]
                        .as_ref()
                        .map(|c| c.borrow().clone())
                        .unwrap_or_default()
                } else {
                    fr.regs[s].clone()
                }
            }
            VarRef::Upval(i) => fr.closure.upvals[*i as usize].borrow().clone(),
            VarRef::Global(name) => self.globals.0.borrow().get(&Val::Str(name.clone())),
        }
    }

    fn assign_var(&self, v: &VarRef, val: Val, fr: &mut Frame<'_>) {
        match v {
            VarRef::Local(s) => {
                let s = *s as usize;
                if fr.captured[s] {
                    match &fr.cells[s] {
                        Some(c) => *c.borrow_mut() = val,
                        None => fr.cells[s] = Some(Rc::new(RefCell::new(val))),
                    }
                } else {
                    fr.regs[s] = val;
                }
            }
            VarRef::Upval(i) => *fr.closure.upvals[*i as usize].borrow_mut() = val,
            VarRef::Global(name) => {
                let _ = self.globals.set(Val::Str(name.clone()), val);
            }
        }
    }

    /// `t[k]`, with string methods for string receivers.
    pub fn index(&mut self, obj: &Val, key: &Val) -> R<Val> {
        match obj {
            Val::Table(t) => {
                let raw = t.get(key);
                if !raw.is_nil() {
                    return Ok(raw);
                }
                let Some(meta) = t.meta() else {
                    return Ok(Val::Nil);
                };
                match meta.get_lstr(&self.k_index) {
                    Val::Nil => Ok(Val::Nil),
                    h @ Val::Table(_) => self.index(&h, key),
                    h => Ok(self
                        .call(&h, vec![obj.clone(), key.clone()])?
                        .into_iter()
                        .next()
                        .unwrap_or_default()),
                }
            }
            Val::Str(_) => Ok(self.string_lib.get(key)),
            other => Err(LuaError::msg(format!(
                "attempt to index a {} value (key '{}')",
                other.type_name(),
                key
            ))),
        }
    }

    pub fn set_index(&mut self, obj: &Val, key: Val, v: Val) -> R<()> {
        match obj {
            Val::Table(t) => {
                if let Some(meta) = t.meta() {
                    let h = meta.get_lstr(&self.k_newindex);
                    if !h.is_nil() && t.get(&key).is_nil() {
                        return match h {
                            Val::Table(_) => self.set_index(&h, key, v),
                            h => self.call(&h, vec![obj.clone(), key, v]).map(|_| ()),
                        };
                    }
                }
                t.set(key, v)
            }
            other => Err(LuaError::msg(format!(
                "attempt to index a {} value (key '{}')",
                other.type_name(),
                key
            ))),
        }
    }

    /// Evaluate to exactly one value.
    fn eval(&mut self, e: &Expr, fr: &mut Frame<'_>) -> R<Val> {
        Ok(match e {
            Expr::Nil => Val::Nil,
            Expr::True => Val::Bool(true),
            Expr::False => Val::Bool(false),
            Expr::Num(n) => Val::Num(*n),
            Expr::Str(s) => Val::Str(s.clone()),
            Expr::Vararg => fr.varargs.first().cloned().unwrap_or_default(),
            Expr::Func(p) => self.make_closure(p, fr),
            Expr::Var(v) => self.read_var(v, fr),
            Expr::Index(o, k) => {
                let o = self.eval(o, fr)?;
                let k = self.eval(k, fr)?;
                self.index(&o, &k)?
            }
            Expr::Call(..) | Expr::Method(..) => self
                .eval_multi(e, fr)?
                .into_iter()
                .next()
                .unwrap_or_default(),
            Expr::Paren(inner) => self.eval(inner, fr)?,
            Expr::And(a, b) => {
                let l = self.eval(a, fr)?;
                if l.truthy() {
                    self.eval(b, fr)?
                } else {
                    l
                }
            }
            Expr::Or(a, b) => {
                let l = self.eval(a, fr)?;
                if l.truthy() {
                    l
                } else {
                    self.eval(b, fr)?
                }
            }
            Expr::Un(op, a) => {
                let v = self.eval(a, fr)?;
                match op {
                    UnOp::Not => Val::Bool(!v.truthy()),
                    UnOp::Neg => Val::Num(-v.to_num().ok_or_else(|| arith_err(&v))?),
                    UnOp::Len => match &v {
                        Val::Str(s) => Val::Num(s.len() as f64),
                        Val::Table(t) => Val::Num(t.len() as f64),
                        other => {
                            return Err(LuaError::msg(format!(
                                "attempt to get length of a {} value",
                                other.type_name()
                            )))
                        }
                    },
                }
            }
            Expr::Bin(op, a, b) => {
                let l = self.eval(a, fr)?;
                let r = self.eval(b, fr)?;
                binop(*op, &l, &r)?
            }
            Expr::Table(fields) => {
                // Positional items keep their `nil` holes (`{nil, x}` has
                // length 2), as Lua's constructor pre-sizes the array part.
                let t = TableRef::new();
                let mut positional: Vec<Val> = Vec::new();
                let last = fields.len();
                for (i, f) in fields.iter().enumerate() {
                    match f {
                        TField::Pos(e) => {
                            if i + 1 == last
                                && matches!(e, Expr::Call(..) | Expr::Method(..) | Expr::Vararg)
                            {
                                positional.extend(self.eval_multi(e, fr)?);
                            } else {
                                positional.push(self.eval(e, fr)?);
                            }
                        }
                        TField::Keyed(k, v) => {
                            let k = self.eval(k, fr)?;
                            let v = self.eval(v, fr)?;
                            t.set(k, v)?;
                        }
                    }
                }
                if !positional.is_empty() {
                    while positional.last().is_some_and(Val::is_nil) {
                        positional.pop();
                    }
                    let mut tb = t.0.borrow_mut();
                    let keyed_arr = std::mem::take(&mut tb.arr);
                    tb.arr = positional;
                    // Keyed integer entries beyond the positional part survive.
                    for (i, v) in keyed_arr.into_iter().enumerate() {
                        if i >= tb.arr.len() && !v.is_nil() {
                            tb.set(Val::Num((i + 1) as f64), v)?;
                        }
                    }
                }
                Val::Table(t)
            }
        })
    }

    /// Evaluate possibly-multi-valued expressions (calls, `...`).
    fn eval_multi(&mut self, e: &Expr, fr: &mut Frame<'_>) -> R<Vec<Val>> {
        match e {
            Expr::Vararg => Ok(fr.varargs.clone()),
            Expr::Call(f, args, line) => {
                let fv = self.eval(f, fr)?;
                let argv = self.eval_all(args, fr)?;
                self.call(&fv, argv)
                    .map_err(|err| annotate(err, &fr.closure.proto, *line, f))
            }
            Expr::Method(obj, name, args, line) => {
                let o = self.eval(obj, fr)?;
                let fv = self
                    .index(&o, &Val::Str(name.clone()))
                    .map_err(|err| annotate_line(err, &fr.closure.proto, *line))?;
                let mut argv = Vec::with_capacity(args.len() + 1);
                argv.push(o);
                argv.extend(self.eval_all(args, fr)?);
                self.call(&fv, argv)
                    .map_err(|err| annotate_line(err, &fr.closure.proto, *line))
            }
            other => Ok(vec![self.eval(other, fr)?]),
        }
    }

    /// Evaluate an expression list, expanding the last one.
    fn eval_all(&mut self, exprs: &[Expr], fr: &mut Frame<'_>) -> R<Vec<Val>> {
        let mut out = Vec::with_capacity(exprs.len());
        for (i, e) in exprs.iter().enumerate() {
            if i + 1 == exprs.len() {
                out.extend(self.eval_multi(e, fr)?);
            } else {
                out.push(self.eval(e, fr)?);
            }
        }
        Ok(out)
    }

    /// Evaluate an expression list adjusted to `n` values.
    fn eval_list(&mut self, exprs: &[Expr], fr: &mut Frame<'_>, n: usize) -> R<Vec<Val>> {
        let mut v = self.eval_all(exprs, fr)?;
        v.resize(n, Val::Nil);
        Ok(v)
    }
}

enum Place {
    Var(VarRef),
    Index(Val, Val),
}

fn annotate_line(err: LuaError, proto: &FuncProto, line: u32) -> LuaError {
    if err.0.contains(" [in ") {
        return err;
    }
    LuaError(format!("{} [in {} line {line}]", err.0, proto.name))
}

fn annotate(err: LuaError, proto: &FuncProto, line: u32, f: &Expr) -> LuaError {
    let _ = f;
    annotate_line(err, proto, line)
}

fn arith_err(v: &Val) -> LuaError {
    LuaError::msg(format!(
        "attempt to perform arithmetic on a {} value",
        v.type_name()
    ))
}

fn arith(l: &Val, r: &Val, f: impl Fn(f64, f64) -> f64) -> R<Val> {
    let a = l.to_num().ok_or_else(|| arith_err(l))?;
    let b = r.to_num().ok_or_else(|| arith_err(r))?;
    Ok(Val::Num(f(a, b)))
}

/// Lua 5.1 `a % b = a - floor(a/b)*b`.
#[must_use]
pub fn lua_mod(a: f64, b: f64) -> f64 {
    a - (a / b).floor() * b
}

fn push_concat(out: &mut Vec<u8>, v: &Val) -> R<()> {
    match v {
        Val::Str(s) => {
            out.extend_from_slice(s.as_bytes());
            Ok(())
        }
        Val::Num(n) => {
            out.extend_from_slice(num_to_string(*n).as_bytes());
            Ok(())
        }
        other => Err(LuaError::msg(format!(
            "attempt to concatenate a {} value",
            other.type_name()
        ))),
    }
}

fn compare(l: &Val, r: &Val) -> R<std::cmp::Ordering> {
    match (l, r) {
        (Val::Num(a), Val::Num(b)) => Ok(a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal)),
        (Val::Str(a), Val::Str(b)) => Ok(a.as_bytes().cmp(b.as_bytes())),
        _ => Err(LuaError::msg(format!(
            "attempt to compare {} with {}",
            l.type_name(),
            r.type_name()
        ))),
    }
}

fn binop(op: BinOp, l: &Val, r: &Val) -> R<Val> {
    use std::cmp::Ordering::{Equal, Greater, Less};
    Ok(match op {
        BinOp::Add => arith(l, r, |a, b| a + b)?,
        BinOp::Sub => arith(l, r, |a, b| a - b)?,
        BinOp::Mul => arith(l, r, |a, b| a * b)?,
        BinOp::Div => arith(l, r, |a, b| a / b)?,
        BinOp::Mod => arith(l, r, lua_mod)?,
        BinOp::Pow => arith(l, r, f64::powf)?,
        BinOp::Concat => {
            let mut out = Vec::new();
            push_concat(&mut out, l)?;
            push_concat(&mut out, r)?;
            Val::bytes(&out)
        }
        BinOp::Eq => Val::Bool(l.raw_eq(r)),
        BinOp::Ne => Val::Bool(!l.raw_eq(r)),
        BinOp::Lt => Val::Bool(lt(l, r)?),
        BinOp::Le => Val::Bool(matches!(compare(l, r)?, Less | Equal) && !nan_involved(l, r)),
        BinOp::Gt => Val::Bool(lt(r, l)?),
        BinOp::Ge => Val::Bool(matches!(compare(l, r)?, Greater | Equal) && !nan_involved(l, r)),
    })
}

fn nan_involved(l: &Val, r: &Val) -> bool {
    matches!(l, Val::Num(a) if a.is_nan()) || matches!(r, Val::Num(b) if b.is_nan())
}

/// `a < b`.
pub fn lt(l: &Val, r: &Val) -> R<bool> {
    Ok(compare(l, r)? == std::cmp::Ordering::Less && !nan_involved(l, r))
}
