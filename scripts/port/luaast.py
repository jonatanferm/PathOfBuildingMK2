"""Lua 5.1 / LuaJIT 2.1 tokenizer, parser and scope resolver (Python 3 stdlib only).

Shared by the P4 port scripts (locals_audit, segments, globals_lint, pairs_inventory).

The grammar is Lua 5.1 plus the LuaJIT 2.1 extensions PoB uses: `goto`/`::label::`,
`\\x`/`\\z`/`\\u{}` string escapes, hex floats and the `LL`/`ULL`/`i` number suffixes.
Every node carries `line` (line of its first token) and `endline` (line of its last
token); positions are 1-based like Lua's.

The resolver follows the Lua scoping rules exactly:
  * `local a, b = e1, e2`: the new locals are visible only after the statement;
  * `local function f`: `f` is visible inside its own body;
  * numeric/generic `for`: the control variables are scoped to the body, the
    expressions are evaluated outside it;
  * `repeat ... until cond`: `cond` sees the body's locals;
  * `function a.b:c()`: implicit `self` parameter;
  * `goto`/labels are not variables.
After `resolve(chunk)`, every `Name` expression node has `.decl` (a `Decl`, or None
for a global), and every `Decl` has `.refs`, the list of `Ref(line, write, node)`.
"""

import sys

sys.setrecursionlimit(max(sys.getrecursionlimit(), 20000))

KEYWORDS = frozenset(
    "and break do else elseif end false for function goto if in local nil not or "
    "repeat return then true until while".split()
)

# Longest first.
# The compound assignments (`+=` ...) are a PoB-runtime LuaJIT extension used once
# upstream (Modules/Main.lua:342); they are parsed so that every PoB file loads.
COMPOUND_OPS = ("+=", "-=", "*=", "/=", "%=", "^=", "..=")
OPS = ["...", "..=", "..", "==", "+=", "-=", "*=", "/=", "%=", "^=", "~=", "<=", ">=", "::",
       "+", "-", "*", "/", "%", "^", "#", "<", ">", "=", "(", ")", "{", "}",
       "[", "]", ";", ":", ",", "."]


class LuaSyntaxError(Exception):
    pass


class Token:
    __slots__ = ("kind", "value", "line", "endline", "pos", "endpos")

    def __init__(self, kind, value, line, endline, pos, endpos):
        self.kind = kind      # 'name' 'kw' 'num' 'str' 'op' 'eof'
        self.value = value
        self.line = line
        self.endline = endline
        self.pos = pos        # byte offset of the first char
        self.endpos = endpos  # byte offset after the last char

    def __repr__(self):
        return "Token(%s,%r,%d)" % (self.kind, self.value, self.line)


_NAME_START = frozenset(b"abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ_")
_NAME_CHARS = frozenset(b"abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ_0123456789")
_DIGITS = frozenset(b"0123456789")
_HEX = frozenset(b"0123456789abcdefABCDEF")
_SPACE = frozenset(b" \t\r\f\v")
_SIMPLE_ESC = {ord("a"): 7, ord("b"): 8, ord("f"): 12, ord("n"): 10, ord("r"): 13,
               ord("t"): 9, ord("v"): 11, ord("\\"): 92, ord('"'): 34, ord("'"): 39}


def _long_bracket_level(src, i):
    """If src[i] starts a long bracket `[=*[`, return its level, else -1."""
    j = i + 1
    n = len(src)
    while j < n and src[j] == 0x3D:  # '='
        j += 1
    if j < n and src[j] == 0x5B:  # '['
        return j - i - 1
    return -1


def tokenize(src):
    """Tokenize Lua source given as bytes. Returns a list of Tokens ending with 'eof'."""
    if isinstance(src, str):
        src = src.encode("latin-1")
    toks = []
    i = 0
    n = len(src)
    line = 1
    # Shebang / '#' first line (luaL_loadfile skips it).
    if n and src[0] == 0x23:
        while i < n and src[i] != 0x0A:
            i += 1
    while True:
        # Skip whitespace and comments.
        while i < n:
            c = src[i]
            if c == 0x0A:
                line += 1
                i += 1
            elif c in _SPACE:
                i += 1
            elif c == 0x2D and i + 1 < n and src[i + 1] == 0x2D:  # '--'
                i += 2
                if i < n and src[i] == 0x5B:
                    lvl = _long_bracket_level(src, i)
                    if lvl >= 0:
                        i, line, _ = _read_long(src, i, lvl, line)
                        continue
                while i < n and src[i] != 0x0A:
                    i += 1
            else:
                break
        if i >= n:
            toks.append(Token("eof", None, line, line, n, n))
            return toks
        c = src[i]
        start = i
        sline = line
        if c in _NAME_START:
            j = i + 1
            while j < n and src[j] in _NAME_CHARS:
                j += 1
            word = src[i:j].decode("latin-1")
            toks.append(Token("kw" if word in KEYWORDS else "name", word, line, line, i, j))
            i = j
        elif c in _DIGITS or (c == 0x2E and i + 1 < n and src[i + 1] in _DIGITS):
            j, val = _read_number(src, i, line)
            toks.append(Token("num", val, line, line, i, j))
            i = j
        elif c == 0x22 or c == 0x27:  # " '
            j, val, line = _read_string(src, i, line)
            toks.append(Token("str", val, sline, line, start, j))
            i = j
        elif c == 0x5B and _long_bracket_level(src, i) >= 0:
            lvl = _long_bracket_level(src, i)
            j, line, val = _read_long(src, i, lvl, line)
            toks.append(Token("str", val, sline, line, start, j))
            i = j
        else:
            for op in OPS:
                ob = op.encode()
                if src.startswith(ob, i):
                    toks.append(Token("op", op, line, line, i, i + len(ob)))
                    i += len(ob)
                    break
            else:
                raise LuaSyntaxError("line %d: unexpected character %r" % (line, chr(c)))


def _read_long(src, i, lvl, line):
    """src[i] is '[' of a long bracket of level lvl. Returns (end, line, text)."""
    close = b"]" + b"=" * lvl + b"]"
    j = i + lvl + 2
    k = src.find(close, j)
    if k < 0:
        raise LuaSyntaxError("line %d: unfinished long string/comment" % line)
    body = src[j:k]
    line += body.count(b"\n")
    # The first newline right after the opening bracket is skipped.
    if body.startswith(b"\r\n") or body.startswith(b"\n\r"):
        body = body[2:]
    elif body[:1] in (b"\n", b"\r"):
        body = body[1:]
    return k + len(close), line, body.decode("latin-1")


def _read_number(src, i, line):
    n = len(src)
    j = i
    if src[j] == 0x30 and j + 1 < n and src[j + 1] in b"xX":
        j += 2
        mant_start = j
        while j < n and (src[j] in _HEX or src[j] == 0x2E):
            j += 1
        mant = src[mant_start:j].decode()
        exp = None
        if j < n and src[j] in b"pP":
            k = j + 1
            if k < n and src[k] in b"+-":
                k += 1
            while k < n and src[k] in _DIGITS:
                k += 1
            exp = src[j + 1:k].decode()
            j = k
        if "." in mant or exp is not None:
            val = float.fromhex("0x" + (mant or "0") + "p" + (exp or "0"))
        else:
            val = int(mant, 16) if mant else 0
    else:
        while j < n and (src[j] in _DIGITS or src[j] == 0x2E):
            j += 1
        if j < n and src[j] in b"eE":
            k = j + 1
            if k < n and src[k] in b"+-":
                k += 1
            while k < n and src[k] in _DIGITS:
                k += 1
            j = k
        text = src[i:j].decode()
        try:
            val = float(text)
        except ValueError:
            raise LuaSyntaxError("line %d: malformed number %r" % (line, text))
        if val.is_integer() and "." not in text and "e" not in text.lower():
            val = int(text)
    # LuaJIT suffixes.
    for suf in (b"ULL", b"ull", b"LL", b"ll", b"i", b"I"):
        if src.startswith(suf, j):
            j += len(suf)
            break
    if j < n and src[j] in _NAME_CHARS:
        raise LuaSyntaxError("line %d: malformed number" % line)
    return j, val


def _read_string(src, i, line):
    q = src[i]
    j = i + 1
    n = len(src)
    out = bytearray()
    while True:
        if j >= n:
            raise LuaSyntaxError("line %d: unfinished string" % line)
        c = src[j]
        if c == q:
            j += 1
            break
        if c == 0x0A:
            raise LuaSyntaxError("line %d: unfinished string" % line)
        if c == 0x5C:  # backslash
            j += 1
            e = src[j]
            if e in _SIMPLE_ESC:
                out.append(_SIMPLE_ESC[e])
                j += 1
            elif e == 0x0A or e == 0x0D:
                out.append(0x0A)
                line += 1
                j += 1
                if j < n and src[j] in (0x0A, 0x0D) and src[j] != e:
                    j += 1
            elif e == ord("x"):
                out.append(int(src[j + 1:j + 3], 16))
                j += 3
            elif e == ord("z"):
                j += 1
                while j < n and (src[j] in _SPACE or src[j] == 0x0A):
                    if src[j] == 0x0A:
                        line += 1
                    j += 1
            elif e == ord("u"):
                k = src.index(b"}", j)
                cp = int(src[j + 2:k], 16)
                out += chr(cp).encode("utf-8")
                j = k + 1
            elif e in _DIGITS:
                k = j
                while k < n and k < j + 3 and src[k] in _DIGITS:
                    k += 1
                out.append(int(src[j:k]) & 0xFF)
                j = k
            else:
                raise LuaSyntaxError("line %d: invalid escape" % line)
        else:
            out.append(c)
            j += 1
    return j, bytes(out).decode("latin-1"), line


# ---------------------------------------------------------------------------
# AST


class Node:
    """Generic AST node: `kind` plus keyword fields; `line`/`endline`, `pos`/`endpos`."""

    def __init__(self, kind, line, **kw):
        self.kind = kind
        self.line = line
        self.endline = line
        self.pos = 0
        self.endpos = 0
        self.__dict__.update(kw)

    def __repr__(self):
        return "<%s@%d>" % (self.kind, self.line)


# Statement kinds: Local LocalFunction Function Assign CallStat Do While Repeat If
#                  NumFor GenFor Return Break Goto Label
# Expression kinds: Nil True False Number String Vararg FunctionExpr Table BinOp UnOp
#                   Name Index Call MethodCall Paren
# Block: Node('Block', stmts=[...])

_BINPRI = {
    "or": (1, 1), "and": (2, 2),
    "<": (3, 3), ">": (3, 3), "<=": (3, 3), ">=": (3, 3), "~=": (3, 3), "==": (3, 3),
    "..": (5, 4),
    "+": (6, 6), "-": (6, 6),
    "*": (7, 7), "/": (7, 7), "%": (7, 7),
    "^": (10, 9),
}
_UNARY_PRI = 8


class Parser:
    def __init__(self, src, name="?"):
        if isinstance(src, str):
            src = src.encode("latin-1")
        self.src = src
        self.name = name
        self.toks = tokenize(src)
        self.i = 0
        self.tok = self.toks[0]
        self.prev = None

    # -- token helpers
    def err(self, msg):
        raise LuaSyntaxError("%s:%d: %s (near %r)" % (self.name, self.tok.line, msg, self.tok.value))

    def advance(self):
        self.prev = self.tok
        self.i += 1
        self.tok = self.toks[self.i]
        return self.prev

    def check(self, kind, value=None):
        t = self.tok
        return t.kind == kind and (value is None or t.value == value)

    def check_op(self, v):
        return self.tok.kind == "op" and self.tok.value == v

    def check_kw(self, v):
        return self.tok.kind == "kw" and self.tok.value == v

    def accept_op(self, v):
        if self.check_op(v):
            return self.advance()
        return None

    def accept_kw(self, v):
        if self.check_kw(v):
            return self.advance()
        return None

    def expect_op(self, v):
        if not self.check_op(v):
            self.err("'%s' expected" % v)
        return self.advance()

    def expect_kw(self, v):
        if not self.check_kw(v):
            self.err("'%s' expected" % v)
        return self.advance()

    def expect_name(self):
        if self.tok.kind != "name":
            self.err("<name> expected")
        return self.advance()

    def start(self, kind, **kw):
        n = Node(kind, self.tok.line, **kw)
        n.pos = self.tok.pos
        return n

    def finish(self, n):
        n.endline = self.prev.endline
        n.endpos = self.prev.endpos
        return n

    # -- blocks
    def parse_chunk(self):
        blk = self.block()
        if self.tok.kind != "eof":
            self.err("'<eof>' expected")
        return blk

    def block_follow(self):
        t = self.tok
        if t.kind == "eof":
            return True
        return t.kind == "kw" and t.value in ("else", "elseif", "end", "until")

    def block(self):
        blk = self.start("Block", stmts=[])
        while not self.block_follow():
            if self.check_kw("return"):
                blk.stmts.append(self.retstat())
                break
            st = self.statement()
            if st is not None:
                blk.stmts.append(st)
        if self.prev is not None and blk.stmts:
            self.finish(blk)
        else:
            blk.endline = blk.line
            blk.endpos = blk.pos
        return blk

    def retstat(self):
        n = self.start("Return")
        self.advance()
        exprs = []
        if not self.block_follow() and not self.check_op(";"):
            exprs = self.exprlist()
        self.accept_op(";")
        n.exprs = exprs
        return self.finish(n)

    def statement(self):
        t = self.tok
        if t.kind == "op":
            if t.value == ";":
                self.advance()
                return None
            if t.value == "::":
                n = self.start("Label")
                self.advance()
                n.label = self.expect_name().value
                self.expect_op("::")
                return self.finish(n)
        if t.kind == "kw":
            v = t.value
            if v == "if":
                return self.ifstat()
            if v == "while":
                n = self.start("While")
                self.advance()
                n.cond = self.expr()
                self.expect_kw("do")
                n.body = self.block()
                self.expect_kw("end")
                return self.finish(n)
            if v == "do":
                n = self.start("Do")
                self.advance()
                n.body = self.block()
                self.expect_kw("end")
                return self.finish(n)
            if v == "for":
                return self.forstat()
            if v == "repeat":
                n = self.start("Repeat")
                self.advance()
                n.body = self.block()
                self.expect_kw("until")
                n.cond = self.expr()
                return self.finish(n)
            if v == "function":
                return self.funcstat()
            if v == "local":
                n = self.start("Local")
                self.advance()
                if self.accept_kw("function"):
                    n.kind = "LocalFunction"
                    nt = self.expect_name()
                    n.name = nt.value
                    n.nameline = nt.line
                    n.func = self.funcbody(nt.line, False)
                    return self.finish(n)
                names = []
                while True:
                    nt = self.expect_name()
                    names.append((nt.value, nt.line))
                    if not self.accept_op(","):
                        break
                n.names = names
                n.exprs = self.exprlist() if self.accept_op("=") else []
                return self.finish(n)
            if v == "return":
                return self.retstat()
            if v == "break":
                n = self.start("Break")
                self.advance()
                return self.finish(n)
            if v == "goto":
                n = self.start("Goto")
                self.advance()
                n.label = self.expect_name().value
                return self.finish(n)
        return self.exprstat()

    def ifstat(self):
        n = self.start("If", clauses=[], orelse=None)
        self.advance()
        cond = self.expr()
        self.expect_kw("then")
        n.clauses.append((cond, self.block()))
        while self.check_kw("elseif"):
            self.advance()
            cond = self.expr()
            self.expect_kw("then")
            n.clauses.append((cond, self.block()))
        if self.accept_kw("else"):
            n.orelse = self.block()
        self.expect_kw("end")
        return self.finish(n)

    def forstat(self):
        n = self.start("For")
        self.advance()
        nt = self.expect_name()
        if self.check_op("="):
            self.advance()
            n.kind = "NumFor"
            n.var = (nt.value, nt.line)
            n.start_ = self.expr()
            self.expect_op(",")
            n.stop = self.expr()
            n.step = self.expr() if self.accept_op(",") else None
        else:
            n.kind = "GenFor"
            names = [(nt.value, nt.line)]
            while self.accept_op(","):
                t2 = self.expect_name()
                names.append((t2.value, t2.line))
            self.expect_kw("in")
            n.names = names
            n.exprs = self.exprlist()
        self.expect_kw("do")
        n.body = self.block()
        self.expect_kw("end")
        return self.finish(n)

    def funcstat(self):
        n = self.start("Function")
        self.advance()
        nt = self.expect_name()
        target = Node("Name", nt.line, name=nt.value)
        target.pos, target.endpos = nt.pos, nt.endpos
        is_method = False
        path = [nt.value]
        while self.check_op(".") or self.check_op(":"):
            sep = self.advance().value
            kt = self.expect_name()
            key = Node("String", kt.line, value=kt.value)
            key.pos, key.endpos = kt.pos, kt.endpos
            idx = Node("Index", nt.line, obj=target, key=key, dot=True)
            idx.pos, idx.endpos = target.pos, kt.endpos
            target = idx
            path.append(kt.value)
            if sep == ":":
                is_method = True
                break
        n.target = target
        n.is_method = is_method
        n.path = path
        n.func = self.funcbody(n.line, is_method)
        return self.finish(n)

    def funcbody(self, line, is_method):
        f = Node("FunctionExpr", line)
        f.pos = self.prev.pos
        params = []
        if is_method:
            params.append(("self", line))
        self.expect_op("(")
        vararg = False
        if not self.check_op(")"):
            while True:
                if self.check_op("..."):
                    self.advance()
                    vararg = True
                    break
                nt = self.expect_name()
                params.append((nt.value, nt.line))
                if not self.accept_op(","):
                    break
        self.expect_op(")")
        f.params = params
        f.is_method = is_method
        f.vararg = vararg
        f.body = self.block()
        self.expect_kw("end")
        return self.finish(f)

    def exprstat(self):
        n = self.start("ExprStat")
        e = self.suffixedexp()
        if self.tok.kind == "op" and self.tok.value in COMPOUND_OPS:
            n.kind = "Assign"
            n.compound = self.advance().value
            n.targets = [e]
            n.exprs = [self.expr()]
            if e.kind not in ("Name", "Index"):
                self.err("syntax error (cannot assign)")
            return self.finish(n)
        if self.check_op("=") or self.check_op(","):
            targets = [e]
            while self.accept_op(","):
                targets.append(self.suffixedexp())
            self.expect_op("=")
            n.kind = "Assign"
            n.compound = None
            n.targets = targets
            n.exprs = self.exprlist()
            for t in targets:
                if t.kind not in ("Name", "Index"):
                    self.err("syntax error (cannot assign)")
            return self.finish(n)
        if e.kind not in ("Call", "MethodCall"):
            self.err("syntax error (statement expected)")
        n.kind = "CallStat"
        n.call = e
        return self.finish(n)

    # -- expressions
    def exprlist(self):
        es = [self.expr()]
        while self.accept_op(","):
            es.append(self.expr())
        return es

    def expr(self, limit=0):
        t = self.tok
        if (t.kind == "kw" and t.value == "not") or (t.kind == "op" and t.value in ("-", "#")):
            n = self.start("UnOp", op=t.value)
            self.advance()
            n.operand = self.expr(_UNARY_PRI)
            e = self.finish(n)
        else:
            e = self.simpleexp()
        while True:
            t = self.tok
            if t.kind in ("op", "kw") and t.value in _BINPRI:
                lp, rp = _BINPRI[t.value]
                if lp <= limit:
                    break
                self.advance()
                rhs = self.expr(rp)
                b = Node("BinOp", e.line, op=t.value, left=e, right=rhs)
                b.pos = e.pos
                e = self.finish(b)
            else:
                break
        return e

    def simpleexp(self):
        t = self.tok
        if t.kind == "num":
            n = self.start("Number", value=t.value)
            self.advance()
            return self.finish(n)
        if t.kind == "str":
            n = self.start("String", value=t.value)
            self.advance()
            return self.finish(n)
        if t.kind == "kw":
            if t.value in ("nil", "true", "false"):
                n = self.start(t.value.capitalize())
                self.advance()
                return self.finish(n)
            if t.value == "function":
                self.advance()
                return self.funcbody(t.line, False)
        if t.kind == "op":
            if t.value == "...":
                n = self.start("Vararg")
                self.advance()
                return self.finish(n)
            if t.value == "{":
                return self.table()
        return self.suffixedexp()

    def primaryexp(self):
        t = self.tok
        if t.kind == "name":
            n = self.start("Name", name=t.value)
            self.advance()
            return self.finish(n)
        if self.check_op("("):
            n = self.start("Paren")
            self.advance()
            n.expr = self.expr()
            self.expect_op(")")
            return self.finish(n)
        self.err("unexpected symbol")

    def suffixedexp(self):
        e = self.primaryexp()
        while True:
            t = self.tok
            if t.kind == "op":
                v = t.value
                if v == ".":
                    self.advance()
                    kt = self.expect_name()
                    key = Node("String", kt.line, value=kt.value)
                    key.pos, key.endpos = kt.pos, kt.endpos
                    n = Node("Index", e.line, obj=e, key=key, dot=True)
                    n.pos = e.pos
                    e = self.finish(n)
                    continue
                if v == "[":
                    self.advance()
                    k = self.expr()
                    self.expect_op("]")
                    n = Node("Index", e.line, obj=e, key=k, dot=False)
                    n.pos = e.pos
                    e = self.finish(n)
                    continue
                if v == ":":
                    self.advance()
                    mt = self.expect_name()
                    n = Node("MethodCall", e.line, obj=e, method=mt.value, methodline=mt.line)
                    n.pos = e.pos
                    n.args = self.funcargs()
                    e = self.finish(n)
                    continue
                if v in ("(", "{"):
                    n = Node("Call", e.line, func=e)
                    n.pos = e.pos
                    n.args = self.funcargs()
                    e = self.finish(n)
                    continue
            elif t.kind == "str":
                n = Node("Call", e.line, func=e)
                n.pos = e.pos
                n.args = self.funcargs()
                e = self.finish(n)
                continue
            return e

    def funcargs(self):
        t = self.tok
        if t.kind == "str":
            n = self.start("String", value=t.value)
            self.advance()
            return [self.finish(n)]
        if self.check_op("{"):
            return [self.table()]
        self.expect_op("(")
        if self.check_op(")"):
            self.advance()
            return []
        args = self.exprlist()
        self.expect_op(")")
        return args

    def table(self):
        n = self.start("Table", fields=[])
        self.expect_op("{")
        while not self.check_op("}"):
            if self.check_op("["):
                self.advance()
                k = self.expr()
                self.expect_op("]")
                self.expect_op("=")
                n.fields.append(("keyed", k, self.expr()))
            elif self.tok.kind == "name" and self.toks[self.i + 1].kind == "op" and self.toks[self.i + 1].value == "=":
                kt = self.advance()
                key = Node("String", kt.line, value=kt.value)
                key.pos, key.endpos = kt.pos, kt.endpos
                self.advance()
                n.fields.append(("named", key, self.expr()))
            else:
                n.fields.append(("pos", None, self.expr()))
            if not (self.accept_op(",") or self.accept_op(";")):
                break
        self.expect_op("}")
        return self.finish(n)


def parse(src, name="?"):
    """Parse Lua source (bytes or str). Returns the chunk's Block node."""
    p = Parser(src, name)
    blk = p.parse_chunk()
    blk.src = p.src
    return blk


# ---------------------------------------------------------------------------
# Generic traversal


def children(n):
    """Yield the direct child nodes of n (statements, expressions, blocks)."""
    k = n.kind
    if k == "Block":
        yield from n.stmts
    elif k == "Local":
        yield from n.exprs
    elif k == "LocalFunction":
        yield n.func
    elif k == "Function":
        yield n.target
        yield n.func
    elif k == "Assign":
        yield from n.targets
        yield from n.exprs
    elif k == "CallStat":
        yield n.call
    elif k == "Do":
        yield n.body
    elif k == "While":
        yield n.cond
        yield n.body
    elif k == "Repeat":
        yield n.body
        yield n.cond
    elif k == "If":
        for c, b in n.clauses:
            yield c
            yield b
        if n.orelse is not None:
            yield n.orelse
    elif k == "NumFor":
        yield n.start_
        yield n.stop
        if n.step is not None:
            yield n.step
        yield n.body
    elif k == "GenFor":
        yield from n.exprs
        yield n.body
    elif k == "Return":
        yield from n.exprs
    elif k == "FunctionExpr":
        yield n.body
    elif k == "Table":
        for _, key, v in n.fields:
            if key is not None:
                yield key
            yield v
    elif k == "BinOp":
        yield n.left
        yield n.right
    elif k == "UnOp":
        yield n.operand
    elif k == "Index":
        yield n.obj
        yield n.key
    elif k == "Call":
        yield n.func
        yield from n.args
    elif k == "MethodCall":
        yield n.obj
        yield from n.args
    elif k == "Paren":
        yield n.expr


def walk(n):
    """Pre-order iteration over n and all its descendants (iterative)."""
    stack = [n]
    while stack:
        x = stack.pop()
        yield x
        ch = list(children(x))
        ch.reverse()
        stack.extend(ch)


def source_text(chunk, node):
    """Original source text of a node (single line, whitespace collapsed)."""
    s = chunk.src[node.pos:node.endpos].decode("latin-1")
    return " ".join(s.split())


# ---------------------------------------------------------------------------
# Scope resolution


class Decl:
    __slots__ = ("name", "line", "kind", "func", "scope", "refs", "node", "init", "uid")
    _counter = 0

    def __init__(self, name, line, kind, func, scope, node=None, init=None):
        self.name = name
        self.line = line
        self.kind = kind      # 'param' 'self' 'local' 'localfn' 'for'
        self.func = func      # enclosing FunctionExpr node, or None for the main chunk
        self.scope = scope    # node owning the scope: Block / NumFor / GenFor / FunctionExpr / Repeat
        self.refs = []
        self.node = node      # declaring statement
        self.init = init      # initialiser expression (for `local x = e`), if any
        Decl._counter += 1
        self.uid = Decl._counter

    def __repr__(self):
        return "Decl(%s@%d,%s)" % (self.name, self.line, self.kind)


class Ref:
    __slots__ = ("line", "write", "node", "func")

    def __init__(self, line, write, node, func):
        self.line = line
        self.write = write
        self.node = node
        self.func = func


class Resolution:
    def __init__(self, chunk):
        self.chunk = chunk
        self.decls = []          # every Decl in source order of declaration
        self.globals = []        # (name, Ref) for every global access
        self.functions = []      # every FunctionExpr (with .parent_func, .decl_name)


def resolve(chunk):
    """Resolve every Name in chunk. Returns a Resolution."""
    res = Resolution(chunk)
    scopes = [{}]  # list of dict name -> Decl (innermost last)
    func_stack = [None]

    def declare(name, line, kind, scope, node=None, init=None):
        d = Decl(name, line, kind, func_stack[-1], scope, node, init)
        scopes[-1][name] = d
        res.decls.append(d)
        return d

    def lookup(name):
        for s in reversed(scopes):
            d = s.get(name)
            if d is not None:
                return d
        return None

    def ref_name(n, write):
        d = lookup(n.name)
        n.decl = d
        r = Ref(n.line, write, n, func_stack[-1])
        if d is None:
            res.globals.append((n.name, r))
        else:
            d.refs.append(r)

    def expr(e):
        # Iterative over long left-recursive chains where possible.
        k = e.kind
        if k == "Name":
            ref_name(e, False)
        elif k == "FunctionExpr":
            function(e)
        elif k in ("Number", "String", "Nil", "True", "False", "Vararg"):
            pass
        elif k == "BinOp":
            # Avoid deep recursion on long `a .. b .. c` / `a + b + c` chains.
            stack = [e]
            while stack:
                x = stack.pop()
                if x.kind == "BinOp":
                    stack.append(x.right)
                    stack.append(x.left)
                else:
                    expr(x)
        else:
            for c in children(e):
                expr(c)

    def target(t):
        if t.kind == "Name":
            ref_name(t, True)
        else:
            expr(t.obj)
            expr(t.key)

    def function(f):
        f.parent_func = func_stack[-1]
        res.functions.append(f)
        func_stack.append(f)
        scopes.append({})
        for idx, (pname, pline) in enumerate(f.params):
            declare(pname, pline, "self" if (f.is_method and idx == 0) else "param", f, f)
        block_inner(f.body)
        scopes.pop()
        func_stack.pop()

    def block(b):
        scopes.append({})
        block_inner(b)
        scopes.pop()

    def block_inner(b):
        for s in b.stmts:
            stmt(s, b)

    def stmt(s, owner):
        k = s.kind
        if k == "Local":
            for e in s.exprs:
                expr(e)
            s.decls = []
            for idx, (name, line) in enumerate(s.names):
                init = s.exprs[idx] if idx < len(s.exprs) else None
                s.decls.append(declare(name, line, "local", owner, s, init))
        elif k == "LocalFunction":
            s.decl = declare(s.name, s.nameline, "localfn", owner, s, s.func)
            s.func.decl_name = s.name
            function(s.func)
        elif k == "Function":
            target(s.target)
            s.func.decl_name = ".".join(s.path[:-1]) + (":" if s.is_method else ".") + s.path[-1] if len(s.path) > 1 else s.path[0]
            function(s.func)
        elif k == "Assign":
            # Lua evaluates the RHS, then assigns; name resolution is not order dependent
            # within a statement, but we visit RHS first for clarity.
            for e in s.exprs:
                expr(e)
            if s.compound and s.targets[0].kind == "Name":
                ref_name(s.targets[0], False)  # `a += b` also reads a
            for t in s.targets:
                target(t)
        elif k == "CallStat":
            expr(s.call)
        elif k == "Do":
            block(s.body)
        elif k == "While":
            expr(s.cond)
            block(s.body)
        elif k == "Repeat":
            scopes.append({})
            block_inner(s.body)
            expr(s.cond)
            scopes.pop()
        elif k == "If":
            for c, b in s.clauses:
                expr(c)
                block(b)
            if s.orelse is not None:
                block(s.orelse)
        elif k == "NumFor":
            expr(s.start_)
            expr(s.stop)
            if s.step is not None:
                expr(s.step)
            scopes.append({})
            s.decls = [declare(s.var[0], s.var[1], "for", s, s)]
            block(s.body)
            scopes.pop()
        elif k == "GenFor":
            for e in s.exprs:
                expr(e)
            scopes.append({})
            s.decls = [declare(nm, ln, "for", s, s) for (nm, ln) in s.names]
            block(s.body)
            scopes.pop()
        elif k == "Return":
            for e in s.exprs:
                expr(e)
        elif k in ("Break", "Goto", "Label"):
            pass
        else:
            raise AssertionError(k)

    block_inner(chunk)
    return res


def parse_file(path):
    with open(path, "rb") as fh:
        src = fh.read()
    chunk = parse(src, str(path))
    return chunk


def function_name_at(res, line):
    """The FunctionExpr whose header is on `line` (the outermost one), or None."""
    best = None
    for f in res.functions:
        if f.line == line and (best is None or f.pos < best.pos):
            best = f
    return best


def enclosing_function_name(f):
    """Human-readable dotted name of the innermost named function enclosing node-func f."""
    while f is not None:
        nm = getattr(f, "decl_name", None)
        if nm:
            return nm
        f = getattr(f, "parent_func", None)
    return "<main>"
