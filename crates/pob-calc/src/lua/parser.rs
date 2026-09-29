//! Lua 5.1 parser producing a slot-resolved AST.
//!
//! Locals are resolved at parse time to per-function slot indices; names not
//! found in any enclosing function are globals. Each `local` declaration gets
//! its own slot (no reuse), and a fresh cell is created each time the
//! declaration executes, so closures capture per-iteration variables like Lua.

use std::rc::Rc;

use super::interp::LuaError;
use super::lexer::{Lexer, Tok};
use super::value::LStr;

#[derive(Debug, Clone)]
pub enum VarRef {
    Local(u32),
    Upval(u32),
    Global(LStr),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BinOp {
    Add,
    Sub,
    Mul,
    Div,
    Mod,
    Pow,
    Concat,
    Eq,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnOp {
    Neg,
    Not,
    Len,
}

#[derive(Debug, Clone)]
pub enum Expr {
    Nil,
    True,
    False,
    Num(f64),
    Str(LStr),
    Vararg,
    Func(Rc<FuncProto>),
    Var(VarRef),
    Index(Box<Expr>, Box<Expr>),
    Call(Box<Expr>, Vec<Expr>, u32),
    Method(Box<Expr>, LStr, Vec<Expr>, u32),
    Bin(BinOp, Box<Expr>, Box<Expr>),
    And(Box<Expr>, Box<Expr>),
    Or(Box<Expr>, Box<Expr>),
    Un(UnOp, Box<Expr>),
    Table(Vec<TField>),
    Paren(Box<Expr>),
}

#[derive(Debug, Clone)]
pub enum TField {
    Pos(Expr),
    Keyed(Expr, Expr),
}

#[derive(Debug)]
pub enum Stat {
    Local(Vec<u32>, Vec<Expr>),
    LocalFunc(u32, Rc<FuncProto>),
    Assign(Vec<Expr>, Vec<Expr>),
    Call(Expr),
    Do(Block),
    While(Expr, Block),
    Repeat(Block, Expr),
    If(Vec<(Expr, Block)>, Option<Block>),
    NumFor {
        var: u32,
        start: Expr,
        limit: Expr,
        step: Option<Expr>,
        body: Block,
    },
    GenFor {
        vars: Vec<u32>,
        exprs: Vec<Expr>,
        body: Block,
    },
    Return(Vec<Expr>),
    Break,
    Goto(LStr),
    Label(LStr),
}

/// Statements plus the source line each one starts on.
#[derive(Debug, Default)]
pub struct Block {
    pub stats: Vec<Stat>,
    pub lines: Vec<u32>,
}

impl std::ops::Deref for Block {
    type Target = [Stat];
    fn deref(&self) -> &[Stat] {
        &self.stats
    }
}

#[derive(Debug, Clone, Copy)]
pub enum UpvalDesc {
    ParentLocal(u32),
    ParentUpval(u32),
}

#[derive(Debug)]
pub struct FuncProto {
    pub name: String,
    /// Chunk (file) the function was defined in.
    pub source: Rc<str>,
    pub params: Vec<u32>,
    pub is_vararg: bool,
    pub nslots: u32,
    /// Slots captured by a nested closure: these live in shared cells;
    /// all others are plain registers.
    pub captured: Vec<bool>,
    pub upvals: Vec<UpvalDesc>,
    pub body: Block,
    pub line: u32,
}

struct FuncState {
    scopes: Vec<Vec<(String, u32)>>,
    nslots: u32,
    captured: Vec<bool>,
    upvals: Vec<(String, UpvalDesc)>,
    is_vararg: bool,
}

impl FuncState {
    fn new(is_vararg: bool) -> Self {
        Self {
            scopes: vec![Vec::new()],
            nslots: 0,
            captured: Vec::new(),
            upvals: Vec::new(),
            is_vararg,
        }
    }

    fn find_local(&self, name: &str) -> Option<u32> {
        for scope in self.scopes.iter().rev() {
            if let Some((_, slot)) = scope.iter().rev().find(|(n, _)| n == name) {
                return Some(*slot);
            }
        }
        None
    }
}

pub struct Parser<'a> {
    lx: Lexer<'a>,
    tok: Tok,
    ahead: Option<Tok>,
    funcs: Vec<FuncState>,
    chunk_name: String,
}

type R<T> = Result<T, LuaError>;

impl<'a> Parser<'a> {
    pub fn new(src: &'a str, chunk_name: &str) -> R<Self> {
        let mut lx = Lexer::new(src);
        let tok = lx.next_tok()?;
        Ok(Self {
            lx,
            tok,
            ahead: None,
            funcs: Vec::new(),
            chunk_name: chunk_name.to_owned(),
        })
    }

    fn err<T>(&self, msg: &str) -> R<T> {
        Err(LuaError::msg(format!(
            "{}:{}: {msg} near {:?}",
            self.chunk_name, self.lx.line, self.tok
        )))
    }

    fn advance(&mut self) -> R<Tok> {
        let next = match self.ahead.take() {
            Some(t) => t,
            None => self.lx.next_tok()?,
        };
        Ok(std::mem::replace(&mut self.tok, next))
    }

    fn peek_ahead(&mut self) -> R<&Tok> {
        if self.ahead.is_none() {
            self.ahead = Some(self.lx.next_tok()?);
        }
        Ok(self.ahead.as_ref().expect("ahead"))
    }

    fn check(&self, t: &Tok) -> bool {
        &self.tok == t
    }

    fn accept(&mut self, t: &Tok) -> R<bool> {
        if self.check(t) {
            self.advance()?;
            Ok(true)
        } else {
            Ok(false)
        }
    }

    fn expect(&mut self, t: &Tok) -> R<()> {
        if self.accept(t)? {
            Ok(())
        } else {
            self.err(&format!("expected {t:?}"))
        }
    }

    fn name(&mut self) -> R<String> {
        match self.advance()? {
            Tok::Name(n) => Ok(n),
            other => {
                self.tok = other;
                self.err("expected name")
            }
        }
    }

    fn fs(&mut self) -> &mut FuncState {
        self.funcs.last_mut().expect("function state")
    }

    fn declare(&mut self, name: String) -> u32 {
        let fs = self.fs();
        let slot = fs.nslots;
        fs.nslots += 1;
        fs.captured.push(false);
        fs.scopes.last_mut().expect("scope").push((name, slot));
        slot
    }

    /// Reserve a slot without making it visible yet (for `local x = x`).
    fn reserve(&mut self) -> u32 {
        let fs = self.fs();
        let slot = fs.nslots;
        fs.nslots += 1;
        fs.captured.push(false);
        slot
    }

    fn activate(&mut self, name: String, slot: u32) {
        self.fs()
            .scopes
            .last_mut()
            .expect("scope")
            .push((name, slot));
    }

    fn resolve(&mut self, name: &str) -> VarRef {
        fn resolve_in(funcs: &mut [FuncState], level: usize, name: &str) -> Option<VarRef> {
            if let Some(slot) = funcs[level].find_local(name) {
                return Some(VarRef::Local(slot));
            }
            if let Some(i) = funcs[level].upvals.iter().position(|(n, _)| n == name) {
                return Some(VarRef::Upval(i as u32));
            }
            if level == 0 {
                return None;
            }
            let desc = match resolve_in(funcs, level - 1, name)? {
                VarRef::Local(s) => {
                    funcs[level - 1].captured[s as usize] = true;
                    UpvalDesc::ParentLocal(s)
                }
                VarRef::Upval(i) => UpvalDesc::ParentUpval(i),
                VarRef::Global(_) => return None,
            };
            let fs = &mut funcs[level];
            fs.upvals.push((name.to_owned(), desc));
            Some(VarRef::Upval((fs.upvals.len() - 1) as u32))
        }
        let top = self.funcs.len() - 1;
        resolve_in(&mut self.funcs, top, name).unwrap_or_else(|| VarRef::Global(LStr::intern(name)))
    }

    // ---- entry points ---------------------------------------------------------

    /// Parse a whole chunk as a vararg main function.
    pub fn parse_chunk(mut self) -> R<Rc<FuncProto>> {
        self.funcs.push(FuncState::new(true));
        let body = self.block()?;
        if !self.check(&Tok::Eof) {
            return self.err("expected <eof>");
        }
        let fs = self.funcs.pop().expect("fs");
        Ok(Rc::new(FuncProto {
            source: self.chunk_name.as_str().into(),
            name: self.chunk_name.clone(),
            params: Vec::new(),
            is_vararg: true,
            nslots: fs.nslots,
            captured: fs.captured,
            upvals: Vec::new(),
            body,
            line: 0,
        }))
    }

    fn block_end(&self) -> bool {
        matches!(
            self.tok,
            Tok::Eof | Tok::End | Tok::Else | Tok::Elseif | Tok::Until
        )
    }

    fn block(&mut self) -> R<Block> {
        self.fs().scopes.push(Vec::new());
        let mut stats = Block::default();
        while !self.block_end() {
            let line = self.lx.line;
            if self.check(&Tok::Return) {
                stats.lines.push(line);
                self.advance()?;
                let exprs = if self.block_end() || self.check(&Tok::Semi) {
                    Vec::new()
                } else {
                    self.expr_list()?
                };
                self.accept(&Tok::Semi)?;
                stats.stats.push(Stat::Return(exprs));
                break;
            }
            if let Some(s) = self.statement()? {
                stats.lines.push(line);
                stats.stats.push(s);
            }
        }
        self.fs().scopes.pop();
        Ok(stats)
    }

    fn statement(&mut self) -> R<Option<Stat>> {
        let line = self.lx.line;
        let _ = line;
        if matches!(&self.tok, Tok::Name(n) if n == "goto")
            && matches!(self.peek_ahead()?, Tok::Name(_))
        {
            self.advance()?;
            let label = self.name()?;
            return Ok(Some(Stat::Goto(LStr::intern(&label))));
        }
        match &self.tok {
            Tok::Semi => {
                self.advance()?;
                Ok(None)
            }
            Tok::If => self.if_stat().map(Some),
            Tok::While => {
                self.advance()?;
                let cond = self.expr()?;
                self.expect(&Tok::Do)?;
                let body = self.block()?;
                self.expect(&Tok::End)?;
                Ok(Some(Stat::While(cond, body)))
            }
            Tok::Do => {
                self.advance()?;
                let body = self.block()?;
                self.expect(&Tok::End)?;
                Ok(Some(Stat::Do(body)))
            }
            Tok::For => self.for_stat().map(Some),
            Tok::Repeat => {
                self.advance()?;
                // The `until` condition sees the body's locals.
                self.fs().scopes.push(Vec::new());
                let mut body = Block::default();
                while !self.block_end() {
                    let line = self.lx.line;
                    if self.check(&Tok::Return) {
                        self.advance()?;
                        let exprs = if self.block_end() {
                            Vec::new()
                        } else {
                            self.expr_list()?
                        };
                        self.accept(&Tok::Semi)?;
                        body.lines.push(line);
                        body.stats.push(Stat::Return(exprs));
                        break;
                    }
                    if let Some(s) = self.statement()? {
                        body.lines.push(line);
                        body.stats.push(s);
                    }
                }
                self.expect(&Tok::Until)?;
                let cond = self.expr()?;
                self.fs().scopes.pop();
                Ok(Some(Stat::Repeat(body, cond)))
            }
            Tok::Function => {
                self.advance()?;
                // funcname: Name {'.' Name} [':' Name]
                let first = self.name()?;
                let mut target = Expr::Var(self.resolve(&first));
                let mut full = first;
                let mut is_method = false;
                loop {
                    if self.accept(&Tok::Dot)? {
                        let n = self.name()?;
                        full.push('.');
                        full.push_str(&n);
                        target =
                            Expr::Index(Box::new(target), Box::new(Expr::Str(LStr::intern(&n))));
                    } else if self.accept(&Tok::Colon)? {
                        let n = self.name()?;
                        full.push(':');
                        full.push_str(&n);
                        target =
                            Expr::Index(Box::new(target), Box::new(Expr::Str(LStr::intern(&n))));
                        is_method = true;
                        break;
                    } else {
                        break;
                    }
                }
                let f = self.func_body(is_method, full)?;
                Ok(Some(Stat::Assign(vec![target], vec![Expr::Func(f)])))
            }
            Tok::Local => {
                self.advance()?;
                if self.accept(&Tok::Function)? {
                    let n = self.name()?;
                    let slot = self.declare(n.clone());
                    let f = self.func_body(false, n)?;
                    return Ok(Some(Stat::LocalFunc(slot, f)));
                }
                let mut names = vec![self.name()?];
                while self.accept(&Tok::Comma)? {
                    names.push(self.name()?);
                }
                let exprs = if self.accept(&Tok::Assign)? {
                    self.expr_list()?
                } else {
                    Vec::new()
                };
                let slots: Vec<u32> = names.iter().map(|_| self.reserve()).collect();
                for (n, s) in names.into_iter().zip(&slots) {
                    self.activate(n, *s);
                }
                Ok(Some(Stat::Local(slots, exprs)))
            }
            Tok::Break => {
                self.advance()?;
                Ok(Some(Stat::Break))
            }
            Tok::DoubleColon => {
                self.advance()?;
                let n = self.name()?;
                self.expect(&Tok::DoubleColon)?;
                Ok(Some(Stat::Label(LStr::intern(&n))))
            }

            _ => {
                let e = self.suffixed_expr()?;
                // Compound assignment (`x += 1`), a LuaJIT extension PoB uses.
                let compound = match self.tok {
                    Tok::Plus => Some(BinOp::Add),
                    Tok::Minus => Some(BinOp::Sub),
                    Tok::Star => Some(BinOp::Mul),
                    Tok::Slash => Some(BinOp::Div),
                    Tok::Percent => Some(BinOp::Mod),
                    Tok::Caret => Some(BinOp::Pow),
                    Tok::Concat => Some(BinOp::Concat),
                    _ => None,
                };
                if let Some(op) = compound {
                    if matches!(self.peek_ahead()?, Tok::Assign)
                        && matches!(e, Expr::Var(_) | Expr::Index(..))
                    {
                        self.advance()?;
                        self.advance()?;
                        let rhs = self.expr()?;
                        let value = Expr::Bin(op, Box::new(e.clone()), Box::new(rhs));
                        return Ok(Some(Stat::Assign(vec![e], vec![value])));
                    }
                }
                if self.check(&Tok::Assign) || self.check(&Tok::Comma) {
                    let mut targets = vec![e];
                    while self.accept(&Tok::Comma)? {
                        targets.push(self.suffixed_expr()?);
                    }
                    self.expect(&Tok::Assign)?;
                    let exprs = self.expr_list()?;
                    for t in &targets {
                        if !matches!(t, Expr::Var(_) | Expr::Index(..)) {
                            return self.err("cannot assign to this expression");
                        }
                    }
                    Ok(Some(Stat::Assign(targets, exprs)))
                } else if matches!(e, Expr::Call(..) | Expr::Method(..)) {
                    Ok(Some(Stat::Call(e)))
                } else {
                    self.err("syntax error (expression statement)")
                }
            }
        }
    }

    fn if_stat(&mut self) -> R<Stat> {
        self.advance()?; // if
        let mut arms = Vec::new();
        let cond = self.expr()?;
        self.expect(&Tok::Then)?;
        arms.push((cond, self.block()?));
        let mut else_block = None;
        loop {
            if self.accept(&Tok::Elseif)? {
                let cond = self.expr()?;
                self.expect(&Tok::Then)?;
                arms.push((cond, self.block()?));
            } else if self.accept(&Tok::Else)? {
                else_block = Some(self.block()?);
                self.expect(&Tok::End)?;
                break;
            } else {
                self.expect(&Tok::End)?;
                break;
            }
        }
        Ok(Stat::If(arms, else_block))
    }

    fn for_stat(&mut self) -> R<Stat> {
        self.advance()?; // for
        let first = self.name()?;
        if self.accept(&Tok::Assign)? {
            let start = self.expr()?;
            self.expect(&Tok::Comma)?;
            let limit = self.expr()?;
            let step = if self.accept(&Tok::Comma)? {
                Some(self.expr()?)
            } else {
                None
            };
            self.expect(&Tok::Do)?;
            self.fs().scopes.push(Vec::new());
            let var = self.declare(first);
            let body = self.block()?;
            self.fs().scopes.pop();
            self.expect(&Tok::End)?;
            return Ok(Stat::NumFor {
                var,
                start,
                limit,
                step,
                body,
            });
        }
        let mut names = vec![first];
        while self.accept(&Tok::Comma)? {
            names.push(self.name()?);
        }
        self.expect(&Tok::In)?;
        let exprs = self.expr_list()?;
        self.expect(&Tok::Do)?;
        self.fs().scopes.push(Vec::new());
        let vars: Vec<u32> = names.into_iter().map(|n| self.declare(n)).collect();
        let body = self.block()?;
        self.fs().scopes.pop();
        self.expect(&Tok::End)?;
        Ok(Stat::GenFor { vars, exprs, body })
    }

    fn func_body(&mut self, is_method: bool, name: String) -> R<Rc<FuncProto>> {
        let line = self.lx.line;
        self.expect(&Tok::LParen)?;
        let mut param_names = Vec::new();
        if is_method {
            param_names.push("self".to_owned());
        }
        let mut is_vararg = false;
        if !self.check(&Tok::RParen) {
            loop {
                if self.accept(&Tok::Ellipsis)? {
                    is_vararg = true;
                    break;
                }
                param_names.push(self.name()?);
                if !self.accept(&Tok::Comma)? {
                    break;
                }
            }
        }
        self.expect(&Tok::RParen)?;
        self.funcs.push(FuncState::new(is_vararg));
        let params: Vec<u32> = param_names.into_iter().map(|n| self.declare(n)).collect();
        let body = self.block()?;
        self.expect(&Tok::End)?;
        let fs = self.funcs.pop().expect("fs");
        Ok(Rc::new(FuncProto {
            source: self.chunk_name.as_str().into(),
            name,
            params,
            is_vararg,
            nslots: fs.nslots,
            captured: fs.captured,
            upvals: fs.upvals.into_iter().map(|(_, d)| d).collect(),
            body,
            line,
        }))
    }

    fn expr_list(&mut self) -> R<Vec<Expr>> {
        let mut v = vec![self.expr()?];
        while self.accept(&Tok::Comma)? {
            v.push(self.expr()?);
        }
        Ok(v)
    }

    fn primary_expr(&mut self) -> R<Expr> {
        match self.tok.clone() {
            Tok::Name(n) => {
                self.advance()?;
                Ok(Expr::Var(self.resolve(&n)))
            }
            Tok::LParen => {
                self.advance()?;
                let e = self.expr()?;
                self.expect(&Tok::RParen)?;
                Ok(Expr::Paren(Box::new(e)))
            }
            _ => self.err("unexpected symbol"),
        }
    }

    fn call_args(&mut self) -> R<Vec<Expr>> {
        match self.tok.clone() {
            Tok::Str(s) => {
                self.advance()?;
                Ok(vec![Expr::Str(LStr::intern_bytes(&s))])
            }
            Tok::LBrace => Ok(vec![self.table_cons()?]),
            Tok::LParen => {
                self.advance()?;
                let args = if self.check(&Tok::RParen) {
                    Vec::new()
                } else {
                    self.expr_list()?
                };
                self.expect(&Tok::RParen)?;
                Ok(args)
            }
            _ => self.err("function arguments expected"),
        }
    }

    fn suffixed_expr(&mut self) -> R<Expr> {
        let mut e = self.primary_expr()?;
        loop {
            let line = self.lx.line;
            match &self.tok {
                Tok::Dot => {
                    self.advance()?;
                    let n = self.name()?;
                    e = Expr::Index(Box::new(e), Box::new(Expr::Str(LStr::intern(&n))));
                }
                Tok::LBracket => {
                    self.advance()?;
                    let k = self.expr()?;
                    self.expect(&Tok::RBracket)?;
                    e = Expr::Index(Box::new(e), Box::new(k));
                }
                Tok::Colon => {
                    self.advance()?;
                    let n = self.name()?;
                    let args = self.call_args()?;
                    e = Expr::Method(Box::new(e), LStr::intern(&n), args, line);
                }
                Tok::LParen | Tok::Str(_) | Tok::LBrace => {
                    let args = self.call_args()?;
                    e = Expr::Call(Box::new(e), args, line);
                }
                _ => return Ok(e),
            }
        }
    }

    fn table_cons(&mut self) -> R<Expr> {
        self.expect(&Tok::LBrace)?;
        let mut fields = Vec::new();
        while !self.check(&Tok::RBrace) {
            if self.check(&Tok::LBracket) {
                self.advance()?;
                let k = self.expr()?;
                self.expect(&Tok::RBracket)?;
                self.expect(&Tok::Assign)?;
                let v = self.expr()?;
                fields.push(TField::Keyed(k, v));
            } else if matches!(self.tok, Tok::Name(_)) && matches!(self.peek_ahead()?, Tok::Assign)
            {
                let n = self.name()?;
                self.expect(&Tok::Assign)?;
                let v = self.expr()?;
                fields.push(TField::Keyed(Expr::Str(LStr::intern(&n)), v));
            } else {
                fields.push(TField::Pos(self.expr()?));
            }
            if !self.accept(&Tok::Comma)? && !self.accept(&Tok::Semi)? {
                break;
            }
        }
        self.expect(&Tok::RBrace)?;
        Ok(Expr::Table(fields))
    }

    fn simple_expr(&mut self) -> R<Expr> {
        let e = match self.tok.clone() {
            Tok::Num(n) => Expr::Num(n),
            Tok::Str(s) => Expr::Str(LStr::intern_bytes(&s)),
            Tok::Nil => Expr::Nil,
            Tok::True => Expr::True,
            Tok::False => Expr::False,
            Tok::Ellipsis => {
                if !self.fs().is_vararg {
                    return self.err("cannot use '...' outside a vararg function");
                }
                Expr::Vararg
            }
            Tok::LBrace => return self.table_cons(),
            Tok::Function => {
                self.advance()?;
                return Ok(Expr::Func(self.func_body(false, "<anonymous>".into())?));
            }
            _ => return self.suffixed_expr(),
        };
        self.advance()?;
        Ok(e)
    }

    pub fn expr(&mut self) -> R<Expr> {
        self.subexpr(0)
    }

    fn subexpr(&mut self, limit: u8) -> R<Expr> {
        const UNARY_PRIORITY: u8 = 8;
        let mut left = match self.tok {
            Tok::Not => {
                self.advance()?;
                Expr::Un(UnOp::Not, Box::new(self.subexpr(UNARY_PRIORITY)?))
            }
            Tok::Minus => {
                self.advance()?;
                let e = self.subexpr(UNARY_PRIORITY)?;
                match e {
                    Expr::Num(n) => Expr::Num(-n),
                    e => Expr::Un(UnOp::Neg, Box::new(e)),
                }
            }
            Tok::Hash => {
                self.advance()?;
                Expr::Un(UnOp::Len, Box::new(self.subexpr(UNARY_PRIORITY)?))
            }
            _ => self.simple_expr()?,
        };
        loop {
            // (left priority, right priority)
            let (op, lp, rp): (Option<BinOp>, u8, u8) = match self.tok {
                Tok::Or => (None, 1, 1),
                Tok::And => (None, 2, 2),
                Tok::Lt => (Some(BinOp::Lt), 3, 3),
                Tok::Gt => (Some(BinOp::Gt), 3, 3),
                Tok::LtEq => (Some(BinOp::Le), 3, 3),
                Tok::GtEq => (Some(BinOp::Ge), 3, 3),
                Tok::NotEq => (Some(BinOp::Ne), 3, 3),
                Tok::EqEq => (Some(BinOp::Eq), 3, 3),
                Tok::Concat => (Some(BinOp::Concat), 5, 4),
                Tok::Plus => (Some(BinOp::Add), 6, 6),
                Tok::Minus => (Some(BinOp::Sub), 6, 6),
                Tok::Star => (Some(BinOp::Mul), 7, 7),
                Tok::Slash => (Some(BinOp::Div), 7, 7),
                Tok::Percent => (Some(BinOp::Mod), 7, 7),
                Tok::Caret => (Some(BinOp::Pow), 10, 9),
                _ => break,
            };
            if lp <= limit {
                break;
            }
            let is_or = matches!(self.tok, Tok::Or);
            self.advance()?;
            let right = self.subexpr(rp)?;
            left = match op {
                Some(op) => Expr::Bin(op, Box::new(left), Box::new(right)),
                None if is_or => Expr::Or(Box::new(left), Box::new(right)),
                None => Expr::And(Box::new(left), Box::new(right)),
            };
        }
        Ok(left)
    }
}

/// Every global name a proto (recursively) reads or writes — for wiring up
/// the environment a chunk expects.
#[must_use]
pub fn collect_globals(proto: &FuncProto) -> std::collections::BTreeSet<String> {
    fn expr(e: &Expr, out: &mut std::collections::BTreeSet<String>) {
        match e {
            Expr::Var(VarRef::Global(n)) => {
                out.insert(n.to_string());
            }
            Expr::Func(p) => block(&p.body, out),
            Expr::Index(a, b) | Expr::Bin(_, a, b) | Expr::And(a, b) | Expr::Or(a, b) => {
                expr(a, out);
                expr(b, out);
            }
            Expr::Call(f, args, _) => {
                expr(f, out);
                args.iter().for_each(|a| expr(a, out));
            }
            Expr::Method(o, _, args, _) => {
                expr(o, out);
                args.iter().for_each(|a| expr(a, out));
            }
            Expr::Un(_, a) | Expr::Paren(a) => expr(a, out),
            Expr::Table(fields) => {
                for f in fields {
                    match f {
                        TField::Pos(v) => expr(v, out),
                        TField::Keyed(k, v) => {
                            expr(k, out);
                            expr(v, out);
                        }
                    }
                }
            }
            _ => {}
        }
    }
    fn block(b: &Block, out: &mut std::collections::BTreeSet<String>) {
        for s in b.iter() {
            match s {
                Stat::Local(_, es) | Stat::Return(es) => es.iter().for_each(|e| expr(e, out)),
                Stat::LocalFunc(_, p) => block(&p.body, out),
                Stat::Assign(ts, es) => ts.iter().chain(es).for_each(|e| expr(e, out)),
                Stat::Call(e) => expr(e, out),
                Stat::Do(b) => block(b, out),
                Stat::While(c, b) | Stat::Repeat(b, c) => {
                    expr(c, out);
                    block(b, out);
                }
                Stat::If(arms, els) => {
                    for (c, b) in arms {
                        expr(c, out);
                        block(b, out);
                    }
                    if let Some(b) = els {
                        block(b, out);
                    }
                }
                Stat::NumFor {
                    start,
                    limit,
                    step,
                    body,
                    ..
                } => {
                    expr(start, out);
                    expr(limit, out);
                    if let Some(s) = step {
                        expr(s, out);
                    }
                    block(body, out);
                }
                Stat::GenFor { exprs, body, .. } => {
                    exprs.iter().for_each(|e| expr(e, out));
                    block(body, out);
                }
                Stat::Break | Stat::Goto(_) | Stat::Label(_) => {}
            }
        }
    }
    let mut out = std::collections::BTreeSet::new();
    block(&proto.body, &mut out);
    out
}

#[cfg(test)]
mod tests {
    #[test]
    fn modparser_parses_and_lists_globals() {
        let path = std::path::Path::new("../../.PathOfBuilding/src/Modules/ModParser.lua");
        let Ok(src) = std::fs::read_to_string(path) else {
            return;
        };
        let proto = super::Parser::new(&src, "ModParser")
            .unwrap()
            .parse_chunk()
            .unwrap();
        let globals = super::collect_globals(&proto);
        eprintln!("GLOBALS: {globals:?}");
    }
}
