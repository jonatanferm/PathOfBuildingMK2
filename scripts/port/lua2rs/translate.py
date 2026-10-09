"""Typed Lua -> Rust expression and statement translation for lua2rs.

The translator handles the table-shaped Lua that ModParser's pattern lists and the
ConfigOptions `apply` closures are written in: constructors of mods/tags/specs, calls
of `mod`/`flag`/`modLib.createMod`/`modList:NewMod`, ModParser helper calls, string
building (`..`, gsub with `string.upper`, firstToUpper, sub, format), arithmetic, and
simple statements (local, assignment to a local, if/elseif/else, return, NewMod calls).
Anything else raises `Untranslatable(reason)`; the caller then emits an `unported!`
placeholder with the Lua source so the owning unit hand-ports it.

Semantics kept from Lua (calc doc §3):
  * `modLib.createMod(name, type, value, ...)` argument dispatch (ModTools.lua:36-51) is
    resolved statically from the Lua types of the extra arguments, including its quirks
    (a number in the source slot is skipped, a table before a number is dropped).
  * Captures are typed from the pattern: Lua patterns have no optional groups, so every
    capture is set on a match; `num` is `tonumber(cap1)` (Option<f64>).
  * nil in arithmetic / concatenation raises LuaError (file-local `nn`/`ns`).
  * Table constructors: nil fields are absent (`vtab` drops them); multiple returns
    expand only in last position (rejected, never silently truncated).
"""

from .common import Untranslatable, rust_f64, rust_ident, rust_str, rust_u32
from .shape import MOD_TYPES
from . import lpat

STRINGY = ("slit", "str", "name")
NUMBERY = ("num", "flags", "skilltype")
NEVER_FALSY = ("num", "flags", "skilltype", "slit", "str", "name", "table", "tag", "mod", "mods")

LTYPE = {
    "num": "number", "flags": "number", "skilltype": "number",
    "slit": "string", "str": "string", "name": "string",
    "bool": "boolean", "nil": "nil", "table": "table", "tag": "table", "mod": "table", "mods": "table",
    "optnum": "number?", "optstr": "string?", "optname": "string?", "optmods": "table?",
}

# File-local helper functions the generated code may need (emitted once per file when used).
FILE_HELPERS = {
    "nn": [
        "/// Lua arithmetic/concatenation on a possibly-nil number: nil raises (calc doc §3.2).",
        "#[inline]",
        "fn nn(v: Option<f64>, what: &str) -> f64 {",
        "    match v { Some(x) => x, None => lua_error!(\"attempt to {} (a nil value)\", what) }",
        "}",
    ],
    "snum": [
        "/// Lua arithmetic on a string operand: coerced like tonumber, else raises (lj_strscan).",
        "#[inline]",
        "fn snum(s: &str, what: &str) -> f64 {",
        "    match tonumber(s) { Some(x) => x, None => lua_error!(\"attempt to {} (a string value)\", what) }",
        "}",
    ],
    "ns": [
        "/// A string capture/local that Lua would index or concatenate: nil raises (calc doc §3.2).",
        "#[inline]",
        "fn ns<'a>(v: Option<&'a str>, what: &str) -> &'a str {",
        "    match v { Some(x) => x, None => lua_error!(\"attempt to {} (a nil value)\", what) }",
        "}",
    ],
    "vtab": [
        "/// A Lua table constructor used as a mod value: nil fields are absent, trailing nils are not",
        "/// part of the array, and string keys are kept in dump order (bytewise; pob-dump FORMAT.md).",
        "fn vtab(mut array: Vec<Value>, fields: Vec<(Name, Value)>) -> Value {",
        "    while matches!(array.last(), Some(Value::Nil)) { array.pop(); }",
        "    let mut fields: Vec<(Name, Value)> = fields.into_iter().filter(|(_, v)| !matches!(v, Value::Nil)).collect();",
        "    fields.sort_by(|a, b| a.0.as_str().as_bytes().cmp(b.0.as_str().as_bytes()));",
        "    Value::Table(Arc::new(ValueTable { array, fields }))",
        "}",
    ],
    "spec_names": [
        "/// A modNameList entry given as a string or as a plain list of names (ModParser.lua:6876-6920",
        "/// reads `modName` as a name list either way).",
        "fn spec_names(names: &[Name]) -> ModSpec {",
        "    ModSpec { modName: Some(names.to_vec()), ..Default::default() }",
        "}",
    ],
    "val_value": [
        "/// The config value `val` stored in a mod (Lua stores whatever value it is).",
        "fn val_value(v: &Val) -> Value {",
        "    match v {",
        "        Val::Nil => Value::Nil,",
        "        Val::Bool(b) => Value::Bool(*b),",
        "        Val::Num(x) => Value::Num(*x),",
        "        Val::Str(s) => Value::Str(AStr::from(&**s)),",
        "        Val::Data(d) => d.clone(),",
        "        Val::Mod(m) => Value::Mod(m.clone()),",
        "        _ => lua_error!(\"lua2rs: a config value that is a mutable table cannot become a mod value\"),",
        "    }",
        "}",
    ],
    "val_str": [
        "/// Lua `..` operand from a config value: numbers use tostring (%.14g), strings are kept.",
        "fn val_str(v: &Val) -> String {",
        "    match v {",
        "        Val::Num(x) => tostring_num(*x),",
        "        Val::Str(s) => s.to_string(),",
        "        _ => lua_error!(\"attempt to concatenate a non-string value\"),",
        "    }",
        "}",
    ],
    "val_eq_str": [
        "/// Lua `val == \"...\"` (no coercion).",
        "fn val_eq_str(v: &Val, s: &str) -> bool { matches!(v, Val::Str(x) if &**x == s) }",
    ],
    "val_eq_num": [
        "/// Lua `val == <number>` (no coercion).",
        "fn val_eq_num(v: &Val, n: f64) -> bool { matches!(v, Val::Num(x) if *x == n) }",
    ],
    "cmp_num": [
        "/// Lua `<`/`<=` operand: numbers only (strings do not coerce in comparisons).",
        "fn cmp_num(v: &Val) -> f64 {",
        "    match v { Val::Num(x) => *x, _ => lua_error!(\"attempt to compare a non-number value with number\") }",
        "}",
    ],
}


class T:
    """A translated (or lazily translatable) Lua expression with a static type."""
    __slots__ = ("code", "ty", "lit", "node", "multi", "parts")

    def __init__(self, code, ty, lit=None, node=None, multi=False, parts=None):
        self.code = code
        self.ty = ty
        self.lit = lit
        self.node = node
        self.multi = multi
        self.parts = parts   # ifexpr: (cond_code, T_then, T_else); orexpr: (T_a, T_b)

    def __repr__(self):
        return "T(%r, %s)" % (self.code, self.ty)


def paren(code):
    """Wrap code in parentheses unless it is atomic or a postfix expression (path, call,
    macro call, index or field chain), which binds tighter than any prefix operator."""
    c = code.strip()
    if _is_postfix(c) or _is_str_lit(c):
        return c
    if c.startswith("(") and c.endswith(")") and _balanced(c[1:-1]):
        return c
    return "(%s)" % c


def _is_ident(c):
    import re
    return re.fullmatch(r"(r#)?[A-Za-z_][A-Za-z0-9_]*", c) is not None


def _is_str_lit(c):
    if len(c) < 2 or c[0] != '"' or c[-1] != '"':
        return False
    i = 1
    while i < len(c) - 1:
        if c[i] == "\\":
            i += 2
            continue
        if c[i] == '"':
            return False
        i += 1
    return i == len(c) - 1


def _is_postfix(c):
    import re
    m = re.match(r"(r#)?[A-Za-z_][A-Za-z0-9_]*(::[A-Za-z_][A-Za-z0-9_]*)*!?", c)
    if not m:
        m = re.match(r"-?[0-9][0-9_]*(\.[0-9]+)?(e-?[0-9]+)?$", c)
        return bool(m) and not c.startswith("-")
    i = m.end()
    n = len(c)
    while i < n:
        ch = c[i]
        if ch in "([":
            j = _group_end(c, i)
            if j < 0:
                return False
            i = j + 1
        elif ch == "." and i + 1 < n and (c[i + 1].isalnum() or c[i + 1] == "_"):
            i += 1
            while i < n and (c[i].isalnum() or c[i] == "_"):
                i += 1
        else:
            return False
    return True


def _group_end(c, i):
    depth = 0
    instr = False
    j = i
    while j < len(c):
        ch = c[j]
        if instr:
            if ch == "\\":
                j += 2
                continue
            if ch == '"':
                instr = False
        elif ch == '"':
            instr = True
        elif ch in "([{":
            depth += 1
        elif ch in ")]}":
            depth -= 1
            if depth == 0:
                return j
        j += 1
    return -1


def _balanced(s):
    depth = 0
    instr = False
    i = 0
    while i < len(s):
        ch = s[i]
        if instr:
            if ch == "\\":
                i += 2
                continue
            if ch == '"':
                instr = False
        elif ch == '"':
            instr = True
        elif ch in "([{":
            depth += 1
        elif ch in ")]}":
            depth -= 1
            if depth < 0:
                return False
        i += 1
    return depth == 0


def _uses_name(node, name):
    from .common import luaast
    for n in luaast.walk(node):
        if n.kind == "Name" and n.name == name:
            return True
    return False


def _assigned_names(node):
    from .common import luaast
    out = set()
    for n in luaast.walk(node):
        if n.kind == "Assign":
            for t in n.targets:
                if t.kind == "Name":
                    out.add(t.name)
    return out


# Names the translator understands only as the file's own top-level locals (aliases and
# helper functions), and names it understands only as real globals.
FILE_LOCAL_FNS = frozenset(("mod", "flag", "m_min", "m_max", "m_floor", "m_ceil", "m_abs", "s_format",
                            "bor", "band", "bnot", "firstToUpper"))
GLOBAL_FNS = frozenset(("tonumber", "tostring", "round", "type"))
GLOBAL_TABLES = frozenset(("ModFlag", "KeywordFlag", "SkillType", "math", "string", "bit", "modLib", "data"))


def scope_of(name_node):
    """'global', 'file' (a local of the main chunk) or 'local' for a resolved Name node."""
    d = getattr(name_node, "decl", None)
    if d is None:
        return "global"
    return "file" if d.func is None else "local"


class Translator:
    def __init__(self, src, shape, mode, flag_names=None):
        self.src = src
        self.shape = shape
        self.mode = mode            # 'modparser' | 'config'
        self.flag_names = flag_names or {}
        self.needs = set()
        self.notes = []   # LUA-QUIRK comments for the entry/fn being translated

    # ------------------------------------------------------------------ utils
    def fail(self, reason, node=None, detail=""):
        raise Untranslatable(reason, detail, getattr(node, "line", None))

    def note(self, text):
        if text not in self.notes:
            self.notes.append(text)

    def S(self, key, **kw):
        tpl = self.shape[key]
        return tpl.format(**kw) if kw else tpl

    def name_lit(self, s):
        return self.S("name_lit", s=rust_str(s))

    # ------------------------------------------------------------ expressions
    def ex(self, n, env):
        k = n.kind
        if k == "Number":
            return T(rust_f64(n.value), "num", lit=n.value)
        if k == "String":
            return T(rust_str(n.value), "slit", lit=n.value)
        if k == "True":
            return T("true", "bool", lit=True)
        if k == "False":
            return T("false", "bool", lit=False)
        if k == "Nil":
            return T("None", "nil")
        if k == "Paren":
            t = self.ex(n.expr, env)
            if t.multi:
                t = T(t.code, t.ty, t.lit, t.node, False, t.parts)
            return t
        if k == "Name":
            if n.name in env:
                return env[n.name]
            return self.global_name(n)
        if k == "Index":
            return self.index(n, env)
        if k == "Call":
            return self.call(n, env)
        if k == "MethodCall":
            return self.method(n, env)
        if k == "BinOp":
            return self.binop(n, env)
        if k == "UnOp":
            return self.unop(n, env)
        if k == "Table":
            return T(None, "table", node=n, parts=env)
        if k == "Vararg":
            self.fail("vararg", n)
        if k == "FunctionExpr":
            self.fail("nested-function", n)
        self.fail("expr-kind", n, k)

    def global_name(self, n):
        nm = n.name
        if nm == "m_huge" and scope_of(n) == "file":
            return T("m_huge", "num")
        self.fail("global" if scope_of(n) == "global" else "upvalue", n, nm)

    def index(self, n, env):
        obj = n.obj
        key = n.key
        if obj.kind == "Name" and obj.name not in env and (
                (obj.name in GLOBAL_TABLES and scope_of(obj) == "global")
                or (obj.name in self.shape["lookups"] and scope_of(obj) == "file")):
            g = obj.name
            if g in ("ModFlag", "KeywordFlag") and key.kind == "String":
                known = self.flag_names.get(g)
                if known is not None and key.value not in known:
                    self.note("LUA-QUIRK: %s.%s does not exist (Data/Global.lua), so Lua reads nil here" % (g, key.value))
                    return T("None", "nil")
                return T(self.S("flag_const", group=g, k=key.value), "flags")
            if g == "SkillType" and key.kind == "String":
                known = self.flag_names.get(g)
                if known is not None and key.value not in known:
                    self.note("LUA-QUIRK: SkillType.%s does not exist (Data/Global.lua), so Lua reads nil here" % key.value)
                    return T("None", "nil")
                return T(self.S("skilltype_const", k=key.value), "skilltype")
            if g == "math" and key.kind == "String" and key.value == "huge":
                return T("m_huge", "num")
            if g in self.shape["lookups"]:
                lk = self.shape["lookups"][g]
                kt = self.ex(key, env)
                return T("%s(%s)" % (lk["rust"], self.as_str_arg(kt, n)), lk["ret"])
        # data.<record>.<Key> / data.<record>["Key"]
        if obj.kind == "Index" and obj.obj.kind == "Name" and obj.obj.name == "data" and "data" not in env \
                and scope_of(obj.obj) == "global" \
                and obj.key.kind == "String" and obj.key.value in self.shape["data_records"] and key.kind == "String":
            return T("%s.%s.%s" % (self.S("data_fn"), obj.key.value, key.value), "num")
        # data.<table>["K"].<field> (shape: data_tables)
        if key.kind == "String" and obj.kind == "Index" and obj.key.kind == "String" and obj.obj.kind == "Index" \
                and obj.obj.obj.kind == "Name" and obj.obj.obj.name == "data" and "data" not in env \
                and scope_of(obj.obj.obj) == "global" \
                and obj.obj.key.kind == "String" and obj.obj.key.value in self.shape["data_tables"]:
            spec = self.shape["data_tables"][obj.obj.key.value]
            if key.value in spec["fields"]:
                code = spec["rust"].format(data=self.S("data_fn"), k=self.name_lit(obj.key.value), f=key.value)
                return T(code, spec["fields"][key.value])
        # build.configTab.input[<lit>] / .x
        if self.mode == "config" and key.kind == "String" and obj.kind == "Index" and obj.key.kind == "String" \
                and obj.key.value == "input" and obj.obj.kind == "Index" and obj.obj.key.kind == "String" \
                and obj.obj.key.value == "configTab" and obj.obj.obj.kind == "Name" and obj.obj.obj.name in env \
                and env[obj.obj.obj.name].ty == "build":
            b = env[obj.obj.obj.name].code
            return T(self.S("config_input_get", build=b, k=rust_str(key.value)), "Val")
        self.fail("index", n, self.src.node_text(n)[:80])

    # -- calls
    def call(self, n, env):
        f = n.func
        args = n.args
        fname = None
        helpers = self.shape["helpers"]
        if f.kind == "Name" and f.name not in env:
            sc = scope_of(f)
            if (f.name in FILE_LOCAL_FNS or f.name in helpers) and sc == "file":
                fname = f.name
            elif f.name in GLOBAL_FNS and sc == "global":
                fname = f.name
        elif f.kind == "Index" and f.obj.kind == "Name" and f.obj.name not in env and f.key.kind == "String" \
                and f.obj.name in GLOBAL_TABLES and scope_of(f.obj) == "global":
            fname = "%s.%s" % (f.obj.name, f.key.value)
        if fname is None:
            self.fail("call-target", n, self.src.node_text(f)[:60])
        if fname == "mod":
            return self.mod_call("mod", args, env, n)
        if fname == "flag":
            return self.mod_call("flag", args, env, n)
        if fname == "modLib.createMod":
            return self.mod_call("createMod", args, env, n)
        if fname == "tonumber":
            if len(args) != 1:
                self.fail("tonumber-base", n)
            return self.tonumber(self.ex(args[0], env), n)
        if fname == "tostring":
            if len(args) != 1:
                self.fail("arity", n, fname)
            t = self.ex(args[0], env)
            if t.ty in NUMBERY:
                return T("tostring_num(%s)" % self.conv(t, "num", n), "str")
            if t.ty in STRINGY:
                return t
            self.fail("tostring-type", n, t.ty)
        if fname in ("bor", "bit.bor", "band", "bit.band"):
            op = "bor" if fname.endswith("bor") else "band"
            if len(args) < 1:
                self.fail("arity", n, fname)
            codes = [self.conv(self.ex(a, env), "u32", a) for a in args]
            acc = codes[0]
            for c in codes[1:]:
                acc = "%s(%s, %s)" % (op, acc, c)
            return T(acc, "flags")
        if fname in ("bnot", "bit.bnot"):
            return T("bnot(%s)" % self.conv(self.ex(args[0], env), "u32", args[0]), "flags")
        if fname in ("m_min", "m_max", "math.min", "math.max"):
            if len(args) < 2:
                self.fail("arity", n, fname)
            fn = "m_min" if fname.endswith("min") else "m_max"
            codes = [self.conv(self.ex(a, env), "mathnum", a) for a in args]
            acc = codes[0]
            for c in codes[1:]:
                acc = "%s(%s, %s)" % (fn, acc, c)
            return T(acc, "num")
        if fname in ("m_floor", "math.floor", "m_ceil", "math.ceil", "m_abs", "math.abs"):
            if len(args) != 1:
                self.fail("arity", n, fname)
            fn = {"floor": "m_floor", "ceil": "m_ceil", "abs": "m_abs"}[fname.split("_")[-1].split(".")[-1]]
            return T("%s(%s)" % (fn, self.conv(self.ex(args[0], env), "mathnum", args[0])), "num")
        if fname == "round":
            if len(args) not in (1, 2):
                self.fail("arity", n, fname)
            v = self.conv(self.ex(args[0], env), "num", args[0])
            if len(args) == 2:
                d = self.ex(args[1], env)
                if d.lit is None or not isinstance(d.lit, int):
                    self.fail("round-dec", n)
                return T("round(%s, Some(%d))" % (v, d.lit), "num")
            return T("round(%s, None)" % v, "num")
        if fname in ("string.format", "s_format"):
            return self.format_call(args, env, n)
        if fname == "string.gsub":
            if len(args) < 3:
                self.fail("arity", n, fname)
            return self.gsub(self.ex(args[0], env), args[1:], env, n)
        if fname in ("string.upper", "string.lower"):
            if len(args) != 1:
                self.fail("arity", n, fname)
            return T("%s(%s)" % (fname.split(".")[1], self.as_str_arg(self.ex(args[0], env), n)), "str")
        if fname == "string.sub":
            return self.sub(self.ex(args[0], env), args[1:], env, n)
        if fname in helpers:
            return self.helper_call(fname, helpers[fname], args, env, n)
        self.fail("call", n, fname)

    def tonumber(self, t, n):
        if t.ty in ("num", "optnum"):
            return t
        if t.ty in ("flags", "skilltype"):
            return T("f64::from(%s)" % t.code, "num")
        if t.ty == "nil":
            return t
        if t.ty == "slit":
            v = _lua_tonumber(t.lit)
            return T(rust_f64(v), "num", lit=v) if v is not None else T("None", "nil")
        if t.ty in ("str", "name"):
            return T("tonumber(%s)" % self.as_str_arg(t, n), "optnum")
        self.fail("tonumber-type", n, t.ty)

    def helper_call(self, fname, spec, args, env, n):
        params = spec["params"]
        out = []
        ai = 0
        for pi, kind in enumerate(params):
            if kind == "tags":
                tags = [self.conv(self.ex(a, env), "tag", a) for a in args[ai:]]
                if args[ai:] and self.ex(args[-1], env).multi:
                    self.fail("multi-return-expansion", n)
                out.append("&[%s]" % ", ".join(tags))
                ai = len(args)
                continue
            a = args[ai] if ai < len(args) else None
            ai += 1
            if a is not None and ai == len(args) and pi == len(params) - 1 and self.ex(a, env).multi:
                self.fail("multi-return-expansion", n)
            t = self.ex(a, env) if a is not None else T("None", "nil")
            out.append(self.helper_arg(t, kind, a or n))
        if ai < len(args):
            self.fail("helper-extra-args", n, fname)
        return T("%s(%s)" % (spec["rust"], ", ".join(out)), spec["ret"])

    def helper_arg(self, t, kind, n):
        if kind == "str":
            if t.ty not in STRINGY:
                self.fail("helper-arg-str", n, t.ty)
            return self.as_str_arg(t, n)
        if kind == "optstr":
            if t.ty == "nil":
                return "None"
            if t.ty in STRINGY:
                return "Some(%s)" % self.as_str_arg(t, n)
            if t.ty == "optstr":
                return t.code
            self.fail("helper-arg-optstr", n, t.ty)
        if kind == "optstring":
            if t.ty == "nil":
                return "None"
            if t.ty in STRINGY:
                return "Some(%s.to_string())" % paren(self.as_str_arg(t, n))
            self.fail("helper-arg-optstring", n, t.ty)
        if kind == "tonumber":
            if t.ty == "nil":
                return "None"
            tn = self.tonumber(t, n)
            if tn.ty == "num":
                return "Some(%s)" % tn.code
            return tn.code if tn.ty == "optnum" else "None"
        if kind == "optnum":
            if t.ty == "nil":
                return "None"
            if t.ty == "num":
                return "Some(%s)" % t.code
            if t.ty == "optnum":
                return t.code
            self.fail("helper-arg-optnum", n, t.ty)
        if kind == "luaarg":
            if t.ty == "nil":
                return "LuaArg::Nil"
            if t.ty in NUMBERY:
                return "LuaArg::Num(%s)" % self.conv(t, "num", n)
            if t.ty == "optnum":
                return "%s.map_or(LuaArg::Nil, LuaArg::Num)" % paren(t.code)
            if t.ty in STRINGY:
                return "LuaArg::Str(%s)" % self.as_str_arg(t, n)
            self.fail("helper-arg-luaarg", n, t.ty)
        if kind == "bool":
            return self.truthy(t, n)
        if kind == "optbool":
            if t.ty == "nil":
                return "None"
            if t.ty == "bool":
                return "Some(%s)" % t.code
            self.fail("helper-arg-optbool", n, t.ty)
        if kind.startswith("opts:"):
            if t.ty == "nil":
                return "None"
            if t.ty != "table":
                self.fail("helper-arg-opts", n, t.ty)
            sname = kind[5:]
            fields = self.shape["helper_structs"][sname]
            parts = []
            for fk, key, v in t.node.fields:
                if fk != "named":
                    self.fail("opts-positional", t.node)
                if key.value not in fields:
                    self.fail("opts-key", t.node, key.value)
                vt = self.ex(v, t.parts)
                if vt.ty == "nil":
                    continue
                fkind = fields[key.value]
                code = self.helper_arg(vt, fkind, v) if fkind != "bool" else self.truthy(vt, v)
                parts.append("%s: %s" % (rust_ident(key.value), code))
            return "Some(%s { %s..Default::default() })" % (sname, "".join(p + ", " for p in parts))
        self.fail("helper-param-kind", n, kind)

    def format_call(self, args, env, n):
        if not args:
            self.fail("arity", n, "string.format")
        fmt = self.ex(args[0], env)
        if fmt.ty != "slit":
            self.fail("format-nonliteral", n)
        parts = [rust_str(fmt.lit)]
        for a in args[1:]:
            t = self.ex(a, env)
            # A multi-value last argument (gsub) only adds arguments string.format ignores.
            if t.ty in NUMBERY:
                parts.append(self.conv(t, "num", a))
            elif t.ty == "optnum":
                parts.append(t.code)
            elif t.ty == "slit":
                parts.append(t.code)
            elif t.ty in ("str", "name"):
                parts.append(self.as_str_arg(t, a))
            elif t.ty == "bool":
                parts.append(t.code)
            else:
                self.fail("format-arg", a, t.ty)
        return T("lfmt!(%s)" % ", ".join(parts), "str")

    def gsub(self, s, rest, env, n):
        if len(rest) not in (2, 3):
            self.fail("arity", n, "gsub")
        p = self.ex(rest[0], env)
        if p.ty != "slit":
            self.fail("gsub-pattern-nonliteral", n)
        sarg = self.as_str_arg(s, n)
        cnt = "None"
        if len(rest) == 3:
            c = self.ex(rest[2], env)
            if c.lit is None or not isinstance(c.lit, int):
                self.fail("gsub-count", n)
            cnt = "Some(%d)" % c.lit
        r = rest[1]
        if r.kind == "Index" and r.obj.kind == "Name" and r.obj.name == "string" and r.key.kind == "String" \
                and r.key.value in ("upper", "lower") and "string" not in env and scope_of(r.obj) == "global":
            code = "gsub_fn(%s, %s, |c| Some(%s(c[0].as_str())), %s).0" % (sarg, p.code, r.key.value, cnt)
            return T(code, "str", multi=True)
        rt = self.ex(r, env)
        if rt.ty == "slit":
            return T("gsub(%s, %s, %s, %s).0" % (sarg, p.code, rt.code, cnt), "str", multi=True)
        self.fail("gsub-repl", n)

    def sub(self, s, rest, env, n):
        if len(rest) not in (1, 2):
            self.fail("arity", n, "sub")

        def ix(a):
            t = self.ex(a, env)
            if t.ty == "num" and isinstance(t.lit, int):
                return "%d" % t.lit
            if t.ty == "num" and isinstance(t.lit, float) and t.lit.is_integer():
                return "%d" % int(t.lit)
            return "(%s as i64)" % paren(self.conv(t, "num", a))
        i = ix(rest[0])
        j = "Some(%s)" % ix(rest[1]) if len(rest) == 2 else "None"
        return T("sub(%s, %s, %s).into_owned()" % (self.as_str_arg(s, n), i, j), "str")

    def method(self, n, env):
        recv = self.ex(n.obj, env)
        m = n.method
        if recv.ty in STRINGY or recv.ty == "optstr":
            if recv.ty == "optstr":
                self.fail("method-on-optstr", n)
            if m == "gsub":
                return self.gsub(recv, n.args, env, n)
            if m in ("lower", "upper"):
                if n.args:
                    self.fail("arity", n, m)
                return T("%s(%s)" % (m, self.as_str_arg(recv, n)), "str")
            if m == "sub":
                return self.sub(recv, n.args, env, n)
            self.fail("string-method", n, m)
        self.fail("method", n, "%s:%s" % (recv.ty, m))

    # -- operators
    def binop(self, n, env):
        op = n.op
        if op in ("+", "-", "*", "/"):
            a = self.conv(self.ex(n.left, env), "num", n.left, "perform arithmetic on")
            b = self.conv(self.ex(n.right, env), "num", n.right, "perform arithmetic on")
            return T("(%s %s %s)" % (a, op, b), "num")
        if op == "%":
            a = self.conv(self.ex(n.left, env), "num", n.left, "perform arithmetic on")
            b = self.conv(self.ex(n.right, env), "num", n.right, "perform arithmetic on")
            return T("lmod(%s, %s)" % (a, b), "num")
        if op == "^":
            a = self.conv(self.ex(n.left, env), "num", n.left, "perform arithmetic on")
            b = self.conv(self.ex(n.right, env), "num", n.right, "perform arithmetic on")
            return T("lpow(%s, %s)" % (a, b), "num")
        if op == "..":
            return self.concat(n, env)
        if op in ("==", "~="):
            code = self.eq(n.left, n.right, env, n)
            return T(code if op == "==" else "!%s" % paren(code), "bool")
        if op in ("<", "<=", ">", ">="):
            a = self.cmp_operand(self.ex(n.left, env), n.left)
            b = self.cmp_operand(self.ex(n.right, env), n.right)
            if a[1] != b[1]:
                self.fail("compare-types", n)
            return T("(%s %s %s)" % (a[0], op, b[0]), "bool")
        if op in ("and", "or"):
            return self.andor(n, env)
        self.fail("binop", n, op)

    def cmp_operand(self, t, n):
        if t.ty in NUMBERY:
            return (self.conv(t, "num", n), "num")
        if t.ty == "optnum":
            return (self.conv(t, "num", n, "compare"), "num")
        if t.ty == "Val":
            self.needs.add("cmp_num")
            return ("cmp_num(&%s)" % paren(t.code), "num")
        if t.ty in STRINGY:
            return (self.as_str_arg(t, n), "str")
        self.fail("compare-operand", n, t.ty)

    def concat(self, n, env):
        parts = []

        def flat(x):
            if x.kind == "BinOp" and x.op == "..":
                flat(x.left)
                flat(x.right)
            else:
                parts.append(x)
        flat(n)
        codes = []
        lits = []
        all_lit = True
        for p in parts:
            t = self.ex(p, env)
            if t.ty == "slit":
                codes.append(t.code)
                lits.append(t.lit)
                continue
            all_lit = False
            if t.ty in ("num", "flags", "skilltype"):
                codes.append(self.conv(t, "num", p))
            elif t.ty == "optnum":
                codes.append(self.conv(t, "num", p, "concatenate"))
            elif t.ty == "name":
                codes.append(t.code)
            elif t.ty == "str":
                codes.append(t.code)
            elif t.ty == "optstr":
                self.needs.add("ns")
                codes.append('ns(%s, "concatenate a nil string")' % t.code)
            elif t.ty == "Val":
                self.needs.add("val_str")
                codes.append("val_str(&%s)" % paren(t.code))
            else:
                self.fail("concat-operand", p, t.ty)
        if all_lit:
            s = "".join(lits)
            return T(rust_str(s), "slit", lit=s)
        return T(self.S("name_cat", parts=", ".join(codes)), "name")

    def eq(self, l, r, env, n):
        # type(x) == "..."
        for a, b in ((l, r), (r, l)):
            if a.kind == "Call" and a.func.kind == "Name" and a.func.name == "type" and "type" not in env \
                    and scope_of(a.func) == "global" \
                    and b.kind == "String" and len(a.args) == 1:
                t = self.ex(a.args[0], env)
                want = b.value
                if t.ty == "Val":
                    variant = {"boolean": "Bool", "number": "Num", "string": "Str", "nil": "Nil", "table": "Table"}.get(want)
                    if variant is None:
                        self.fail("type-name", n, want)
                    if variant == "Nil":
                        return "matches!(%s, Val::Nil)" % t.code
                    return "matches!(%s, Val::%s(_))" % (t.code, variant)
                lt = LTYPE.get(t.ty)
                if lt is None or lt.endswith("?"):
                    self.fail("type-of", n, t.ty)
                return "true" if lt == want else "false"
        a = self.ex(l, env)
        b = self.ex(r, env)
        if a.ty in ("slit", "num", "nil", "bool") and b.ty not in ("slit", "num", "nil", "bool"):
            a, b = b, a
        if a.ty == "Val":
            if b.ty == "slit":
                self.needs.add("val_eq_str")
                return "val_eq_str(&%s, %s)" % (paren(a.code), b.code)
            if b.ty == "num":
                self.needs.add("val_eq_num")
                return "val_eq_num(&%s, %s)" % (paren(a.code), b.code)
            if b.ty == "nil":
                return "matches!(%s, Val::Nil)" % a.code
            if b.ty == "bool":
                return "matches!(%s, Val::Bool(%s))" % (a.code, b.code)
            self.fail("eq-val", n, b.ty)
        if b.ty == "nil":
            if a.ty in ("optnum", "optstr", "optname", "optmods"):
                return "%s.is_none()" % paren(a.code)
            if a.ty == "nil":
                return "true"
            self.fail("eq-nil", n, a.ty)
        if a.ty in STRINGY and b.ty in STRINGY:
            return "(%s == %s)" % (self.as_str_arg(a, n), self.as_str_arg(b, n))
        if a.ty == "optstr" and b.ty in STRINGY:
            return "(%s == Some(%s))" % (a.code, self.as_str_arg(b, n))
        if a.ty in NUMBERY and b.ty in NUMBERY:
            return "(%s == %s)" % (self.conv(a, "num", n), self.conv(b, "num", n))
        if a.ty == "optnum" and b.ty in NUMBERY:
            return "(%s == Some(%s))" % (a.code, self.conv(b, "num", n))
        if a.ty == "bool" and b.ty == "bool":
            return "(%s == %s)" % (a.code, b.code)
        self.fail("eq-types", n, "%s == %s" % (a.ty, b.ty))

    def andor(self, n, env):
        op = n.op
        if op == "or" and n.left.kind == "BinOp" and n.left.op == "and":
            # a and b or c
            cond = self.cond(n.left.left, env)
            tb = self.ex(n.left.right, env)
            if tb.ty not in NEVER_FALSY:
                self.fail("and-or-falsy-branch", n, tb.ty)
            tc = self.ex(n.right, env)
            return T(None, "ifexpr", parts=(cond, tb, tc), node=n)
        a = self.ex(n.left, env)
        b = self.ex(n.right, env)
        if op == "or":
            if a.ty in NEVER_FALSY:
                return a
            if a.ty in ("optnum", "optstr", "optname", "Val", "nil", "bool"):
                return T(None, "orexpr", parts=(a, b), node=n)
        if a.ty == "bool" and b.ty == "bool":
            return T("(%s %s %s)" % (a.code, "&&" if op == "and" else "||", b.code), "bool")
        self.fail("and-or", n, "%s %s %s" % (a.ty, op, b.ty))

    def unop(self, n, env):
        op = n.op
        if op == "-":
            t = self.ex(n.operand, env)
            if t.ty == "num" and t.lit is not None:
                v = -t.lit
                return T(rust_f64(v), "num", lit=v)
            return T("(-%s)" % paren(self.conv(t, "num", n.operand, "perform arithmetic on")), "num")
        if op == "not":
            return T("!%s" % paren(self.cond(n.operand, env)), "bool")
        if op == "#":
            t = self.ex(n.operand, env)
            if t.ty in STRINGY:
                return T("(len(%s) as f64)" % self.as_str_arg(t, n), "num")
            self.fail("len-of", n, t.ty)
        self.fail("unop", n, op)

    # ------------------------------------------------------------ conversions
    def as_str_arg(self, t, n):
        """A `&str` argument."""
        if t.ty == "slit":
            return t.code
        if t.ty == "str" and t.lit == "&str":
            return t.code
        if t.ty in ("str", "name"):
            return "&*%s" % paren(t.code)
        if t.ty == "optstr":
            self.fail("nil-possible-string", n)
        if t.ty in ("ifexpr",):
            c, a, b = t.parts
            return "if %s { %s } else { %s }" % (c, self.as_str_arg(a, n), self.as_str_arg(b, n))
        self.fail("str-arg", n, t.ty)

    def owned_string(self, t, n):
        """A Rust `String` for a string-typed T. Every 'str' producer other than a bound
        capture (`lit == "&str"`) already yields a String (gsub().0, lfmt!, firstToUpper, ...)."""
        if t.ty == "str" and t.lit != "&str":
            return t.code
        if t.ty == "name":
            return "%s.to_string()" % paren(t.code)
        return "%s.to_string()" % paren(self.as_str_arg(t, n))

    def truthy(self, t, n):
        ty = t.ty
        if ty == "bool":
            return t.code
        if ty in NEVER_FALSY:
            return "true"
        if ty == "nil":
            return "false"
        if ty in ("optnum", "optstr", "optname", "optmods"):
            return "%s.is_some()" % paren(t.code)
        if ty == "Val":
            return "%s.truthy()" % paren(t.code)
        self.fail("truthy", n, ty)

    def cond(self, n, env):
        if n.kind == "BinOp" and n.op in ("and", "or"):
            return "(%s %s %s)" % (self.cond(n.left, env), "&&" if n.op == "and" else "||", self.cond(n.right, env))
        if n.kind == "UnOp" and n.op == "not":
            return "!%s" % paren(self.cond(n.operand, env))
        if n.kind == "Paren":
            return self.cond(n.expr, env)
        t = self.ex(n, env)
        if t.ty == "ifexpr":
            c, a, b = t.parts
            return "(if %s { %s } else { %s })" % (c, self.truthy(a, n), self.truthy(b, n))
        return self.truthy(t, n)

    def conv(self, t, want, n, what=None):
        """Render T as Rust code of the wanted kind."""
        ty = t.ty
        if ty == "ifexpr":
            c, a, b = t.parts
            return "if %s { %s } else { %s }" % (c, self.conv(a, want, n, what), self.conv(b, want, n, what))
        if ty == "orexpr":
            return self.conv_or(t, want, n)
        if want in ("num", "mathnum"):
            if ty == "num":
                return t.code
            if ty in ("flags", "skilltype"):
                return "f64::from(%s)" % t.code
            if ty == "optnum":
                self.needs.add("nn")
                verb = what or "perform arithmetic on"
                if _is_ident(t.code):
                    return 'nn(%s, "%s local \'%s\'")' % (t.code, verb, t.code)
                return 'nn(%s, "%s a nil value")' % (t.code, verb)
            if ty == "Val":
                return "%s.num()" % paren(t.code)
            if ty == "str":
                self.needs.add("snum")
                return 'snum(%s, "%s a string")' % (self.as_str_arg(t, n), what or "perform arithmetic on")
            self.fail("num-operand", n, ty)
        if want == "u32":
            if ty == "flags":
                return t.code
            if ty == "num" and t.lit is not None:
                return rust_u32(t.lit)
            self.fail("flags-operand", n, ty)
        if want == "name":
            if ty == "slit":
                return self.name_lit(t.lit)
            if ty == "name":
                return t.code
            if ty == "str":
                return self.S("name_from_str", e=self.as_str_arg(t, n))
            self.fail("name-operand", n, ty)
        if want == "value":
            return self.conv_value(t, n)
        if want == "tag":
            if ty == "tag":
                return t.code
            if ty == "table":
                return self.tag(t.node, t.parts)
            self.fail("tag-operand", n, ty)
        if want == "mod":
            if ty == "mod":
                return t.code
            self.fail("mod-operand", n, ty)
        if want == "mods":
            if ty == "mods":
                return t.code
            if ty == "table":
                return self.mods_table(t.node, t.parts)
            self.fail("mods-operand", n, ty)
        if want == "optmods":
            if ty == "nil":
                return "None"
            if ty in ("mods", "table"):
                return "Some(%s)" % self.conv(t, "mods", n)
            if ty == "optmods":
                return t.code
            self.fail("optmods-operand", n, ty)
        if want == "spec":
            if ty == "table":
                return self.spec(t.node, t.parts)
            self.fail("spec-operand", n, ty)
        if want == "Val":
            if ty == "Val":
                return "%s.clone()" % paren(t.code)
            if ty in ("num", "bool", "slit"):
                return "Val::from(%s)" % t.code
            self.fail("val-operand", n, ty)
        self.fail("conv", n, "%s -> %s" % (ty, want))

    def conv_or(self, t, want, n):
        a, b = t.parts
        if want in ("num", "mathnum") and a.ty == "optnum":
            return "%s.unwrap_or(%s)" % (paren(a.code), self.conv(b, want, n))
        if want in ("num", "mathnum") and a.ty == "Val" and b.ty == "num" and b.lit is not None:
            return "%s.num_or(%s)" % (paren(a.code), b.code)
        if want == "value" and a.ty == "optnum" and b.ty == "num":
            return "%s.unwrap_or(%s)" % (paren(a.code), b.code)
        if want == "value" and a.ty == "Val":
            # `x or d` keeps x (whatever it holds) unless it is nil/false
            self.needs.add("val_value")
            return "{ let v = %s; if v.truthy() { val_value(&v) } else { %s } }" % (
                a.code, self.S("value_from", e=self.conv_value(b, n)))
        self.fail("or-conv", n, "%s or %s -> %s" % (a.ty, b.ty, want))

    def conv_value(self, t, n):
        """An `impl Into<Value>` expression (mod value / table field)."""
        ty = t.ty
        if ty in ("num", "optnum", "slit", "bool", "name"):
            return t.code
        if ty in ("flags", "skilltype"):
            return "f64::from(%s)" % t.code
        if ty == "str":
            return "&*%s" % paren(t.code)
        if ty == "nil":
            return self.S("value_nil")
        if ty == "optname":
            return t.code
        if ty == "mod":
            return t.code
        if ty == "Val":
            self.needs.add("val_value")
            return "val_value(&%s)" % paren(t.code)
        if ty == "table":
            return self.vtab(t.node, t.parts)
        self.fail("value-operand", n, ty)

    # ------------------------------------------------------------ constructors
    def vtab(self, node, env):
        self.needs.add("vtab")
        arr = []
        fields = []
        npos = sum(1 for f in node.fields if f[0] == "pos")
        seen = 0
        for fk, key, v in node.fields:
            vt = self.ex(v, env)
            if fk == "pos":
                seen += 1
                if vt.multi and seen == npos:
                    self.fail("multi-return-expansion", v)
                arr.append(self.S("value_from", e=self.conv_value(vt, v)))
            elif fk == "named":
                if vt.ty == "nil":
                    continue
                fields.append(self.S("vtab_field", k=rust_str(key.value), v=self.S("value_from", e=self.conv_value(vt, v))))
            else:
                self.fail("keyed-field", v)
        return self.S("vtab_call", array=", ".join(arr), fields=", ".join(fields))

    def mods_table(self, node, env):
        items = []
        npos = 0
        for fk, key, v in node.fields:
            if fk != "pos":
                self.fail("mods-table-named-field", v)
            npos += 1
        for i, (fk, key, v) in enumerate(node.fields):
            vt = self.ex(v, env)
            if vt.multi and i == npos - 1:
                self.fail("multi-return-expansion", v)
            if vt.ty != "mod":
                self.fail("mods-table-element", v, vt.ty)
            items.append(vt.code)
        return self.S("mods_vec", items=", ".join(items))

    def tag(self, node, env):
        d = {}
        order = []
        for fk, key, v in node.fields:
            if fk != "named":
                self.fail("tag-positional", node)
            d[key.value] = v
            order.append(key.value)
        if "type" not in d or d["type"].kind != "String":
            self.fail("tag-without-type", node)
        ttype = d["type"].value
        if ttype not in self.shape["tag_types"]:
            self.fail("tag-type", node, ttype)
        kinds = self.shape["tag_fields"]
        parts = []
        for k in order:
            if k == "type":
                continue
            if k not in kinds:
                self.fail("tag-key", node, k)
            code = self.tag_field(kinds[k], d[k], k, env)
            if code is not None:
                parts.append("%s = %s" % (rust_ident(k), code))
        fields = "".join(", " + p for p in parts)
        return self.S("tag", t=self.S("tag_type", t=ttype), fields=fields)

    def tag_field(self, kind, v, k, env):
        t = self.ex(v, env)
        if t.ty == "nil":
            return None
        if kind == "name":
            if t.ty == "slit":
                return t.code
            if t.ty in ("name", "optname"):
                return t.code
            if t.ty == "str":
                return self.S("name_from_str", e=self.as_str_arg(t, v))
            self.fail("tag-field-name", v, "%s=%s" % (k, t.ty))
        if kind == "astr":
            if t.ty == "slit":
                return t.code
            if t.ty in ("str", "name"):
                return "&*%s" % paren(t.code)
            self.fail("tag-field-str", v, "%s=%s" % (k, t.ty))
        if kind == "num":
            if t.ty in ("num", "optnum"):
                return t.code
            if t.ty in ("flags", "skilltype"):
                return "f64::from(%s)" % t.code
            if t.ty == "str" and k in self.shape["tag_num_from_string"]:
                self.note("LUA-QUIRK: tag.%s holds a capture string in Lua; EvalMod only uses it in arithmetic, "
                          "which coerces it like tonumber" % k)
                self.needs.add("snum")
                return 'snum(%s, "perform arithmetic on tag.%s")' % (self.as_str_arg(t, v), k)
            self.fail("tag-field-num", v, "%s=%s" % (k, t.ty))
        if kind in ("bool", "optbool"):
            if t.ty == "bool":
                return t.code
            self.fail("tag-field-bool", v, "%s=%s" % (k, t.ty))
        if kind == "u32":
            return self.conv(t, "u32", v)
        if kind == "skilltype":
            if t.ty == "skilltype":
                return t.code
            self.fail("tag-field-skilltype", v, "%s=%s" % (k, t.ty))
        if kind in ("namelist", "numlist", "skilltypelist"):
            if t.ty != "table":
                self.fail("tag-field-list", v, "%s=%s" % (k, t.ty))
            elems = []
            for fk, key, ev in t.node.fields:
                if fk != "pos":
                    self.fail("tag-field-list-named", v, k)
                et = self.ex(ev, env)
                if et.multi:
                    self.fail("multi-return-expansion", ev)
                if kind == "namelist":
                    elems.append(self.conv(et, "name", ev))
                elif kind == "numlist":
                    elems.append(self.conv(et, "num", ev))
                else:
                    if et.ty != "skilltype":
                        self.fail("tag-field-skilltype", ev, et.ty)
                    elems.append(et.code)
            return "&[%s][..]" % ", ".join(elems)
        if kind == "ramp":
            if t.ty != "table":
                self.fail("tag-field-ramp", v)
            rows = []
            for fk, key, ev in t.node.fields:
                if fk != "pos" or ev.kind != "Table" or len(ev.fields) != 2:
                    self.fail("tag-field-ramp", v)
                pair = [self.conv(self.ex(x[2], env), "num", x[2]) for x in ev.fields]
                rows.append("[%s, %s]" % tuple(pair))
            return "&[%s][..]" % ", ".join(rows)
        if kind == "sockets":
            if t.ty == "slit" and t.lit == "all":
                return "Sockets::All"
            if t.ty == "num":
                return "Sockets::Count(%s)" % t.code
            if t.ty == "table":
                elems = []
                for fk, key, ev in t.node.fields:
                    if fk != "pos":
                        self.fail("tag-field-sockets", v)
                    elems.append(self.conv(self.ex(ev, env), "num", ev))
                return "Sockets::List(Arc::from(&[%s][..]))" % ", ".join(elems)
            self.fail("tag-field-sockets", v, t.ty)
        self.fail("tag-field-kind", v, kind)

    def spec(self, node, env, names_only_helper=True):
        """A ModParser "misc" table -> ModSpec (doc §6)."""
        kinds = self.shape["spec_fields"]
        names = []
        parts = []
        extra = []
        for fk, key, v in node.fields:
            if fk == "pos":
                t = self.ex(v, env)
                if t.multi:
                    self.fail("multi-return-expansion", v)
                names.append(self.conv(t, "name", v))
                continue
            if fk != "named":
                self.fail("keyed-field", v)
            k = key.value
            t = self.ex(v, env)
            if t.ty == "nil":
                continue
            kind = kinds.get(k)
            if kind is None:
                extra.append("(%s, %s)" % (self.name_lit(k), self.S("value_from", e=self.conv_value(t, v))))
                continue
            if kind == "u32":
                code = self.conv(t, "u32", v)
            elif kind == "opttag":
                code = "Some(%s)" % self.conv(t, "tag", v)
            elif kind == "tags":
                if t.ty != "table":
                    self.fail("spec-taglist", v, t.ty)
                tags = []
                for fk2, key2, ev in t.node.fields:
                    if fk2 != "pos":
                        self.fail("spec-taglist", v)
                    tags.append(self.conv(self.ex(ev, env), "tag", ev))
                code = "vec![%s]" % ", ".join(tags)
            elif kind == "bool":
                if t.ty != "bool":
                    self.fail("spec-bool", v, "%s=%s" % (k, t.ty))
                code = t.code
            elif kind == "optname":
                code = "Some(%s)" % self.conv(t, "name", v)
            elif kind == "optmodtype":
                if t.ty != "slit" or t.lit not in MOD_TYPES:
                    self.fail("spec-modtype", v)
                code = "Some(%s)" % self.S("modtype", t=t.lit)
            else:
                self.fail("spec-kind", v, kind)
            parts.append("%s: %s" % (rust_ident(k), code))
        if names:
            if not parts and not extra and names_only_helper:
                self.needs.add("spec_names")
                return "%s(&[%s])" % (self.shape["spec_names_fn"], ", ".join(names))
            parts.insert(0, "modName: Some(vec![%s])" % ", ".join(names))
        if extra:
            parts.append("extra: vec![%s]" % ", ".join(extra))
        return self.S("spec_struct", fields="".join(p + ", " for p in parts))

    # -- mod constructors (modLib.createMod argument dispatch)
    def lua_type_of(self, a, env):
        t = self.ex(a, env)
        if t.ty in ("ifexpr", "orexpr"):
            return t, "?"
        return t, LTYPE.get(t.ty, "?")

    def resolve_createmod(self, rest, env, n):
        """ModTools.lua:36-51 on the static Lua types of `...`. Returns (source, flags, kw, tags)."""
        typed = [self.lua_type_of(a, env) for a in rest]
        cnt = len(rest)

        def is_(i, lt):
            t, ty = typed[i]
            if ty.endswith("?") or ty == "?":
                self.fail("createMod-ambiguous-arg", rest[i], t.ty)
            return ty == lt
        source = None
        flags = None
        kw = None
        tag_start = 1
        if cnt >= 1 and is_(0, "string"):
            source = typed[0][0]
            tag_start = 2
        if cnt >= 2 and is_(1, "number"):
            flags = typed[1][0]
            tag_start = 3
            if typed[0][1] == "number" and not (typed[0][0].lit == 0 and typed[1][0].lit == 0):
                self.note("LUA-QUIRK: modLib.createMod skips the number in the source slot, so the next "
                          "number is the flags and the one after it the keywordFlags")
        if cnt >= 3 and is_(2, "number"):
            kw = typed[2][0]
            tag_start = 4
        tags = list(range(tag_start - 1, cnt))
        while tags and typed[tags[-1]][1] == "nil":
            tags.pop()
        holes = [j for j, i in enumerate(tags) if typed[i][1] == "nil"]
        if holes:
            # `{ ..., select(tagStart, ...) }` puts nil at mod[k]: ipairs(mod) (EvalMod, the only
            # tag reader in the calc) stops there, so the later tags never apply.
            dropped = sum(1 for i in tags[holes[0]:] if typed[i][1] != "nil")
            tags = tags[:holes[0]]
            self.note("LUA-QUIRK: modLib.createMod stores nil at mod[%d] (a nil source/flags slot that is "
                      "not a string/number), so ipairs(mod) never reaches the %d tag(s) after it; they are "
                      "dropped" % (holes[0] + 1, dropped))
        for i in tags:
            t, ty = typed[i]
            if ty != "table" or t.ty == "mods":
                self.fail("createMod-tag-slot", rest[i], ty)
        if tags and typed[tags[-1]][0].multi:
            self.fail("multi-return-expansion", n)
        return source, flags, kw, [typed[i][0] for i in tags], [rest[i] for i in tags]

    def mod_call(self, kind, args, env, n, obj=None):
        if kind == "flag":
            if len(args) < 1:
                self.fail("arity", n, "flag")
            name_a, rest = args[0], args[1:]
            ty_code = None
            value = None
        else:
            if len(args) < 3:
                self.fail("arity", n, kind)
            name_a, ty_a, val_a, rest = args[0], args[1], args[2], args[3:]
            tyt = self.ex(ty_a, env)
            if tyt.ty != "slit" or tyt.lit not in MOD_TYPES:
                self.fail("mod-type", ty_a, str(tyt.lit))
            ty_code = self.S("modtype", t=tyt.lit)
            vt = self.ex(val_a, env)
            if vt.multi and not rest:
                self.fail("multi-return-expansion", val_a)
            value = self.conv(vt, "value", val_a)
        name = self.conv(self.ex(name_a, env), "name", name_a)
        source, flags, kw, tagts, tagnodes = self.resolve_createmod(rest, env, n)
        if source is not None:
            src_code = self.as_str_arg(source, n)
        else:
            src_code = None
        flags_code = self.conv(flags, "u32", n) if flags is not None else "0"
        kw_code = self.conv(kw, "u32", n) if kw is not None else "0"
        tags = ", ".join(self.conv(t, "tag", tn) for t, tn in zip(tagts, tagnodes))
        if kind == "NewMod" and src_code is None:
            # ModStore.lua:109-111: NewMod(...) is AddMod(createMod(...)); the contract's NewMod
            # takes `source: &str`, so a nil source goes through AddMod.
            mod = self.S("mod_call", fn=self.shape["createMod_fn"], name=name, ty=ty_code, value=value,
                         source=self.S("source_none"), flags=flags_code, kw=kw_code, tags=tags)
            return T(self.S("addmod_call", obj=obj, mod=mod), "unit")
        if kind == "NewMod":
            code = self.S("newmod_call", obj=obj, name=name, ty=ty_code, value=value, source=src_code,
                          flags=flags_code, kw=kw_code, tags=tags)
            return T(code, "unit")
        source_opt = self.S("source_some", s=src_code) if src_code is not None else self.S("source_none")
        if kind == "flag":
            code = self.S("flag_call", fn=self.shape["flag_fn"], name=name, source=source_opt,
                          flags=flags_code, kw=kw_code, tags=tags)
        else:
            fn = self.shape["mod_fn"] if kind == "mod" else self.shape["createMod_fn"]
            code = self.S("mod_call", fn=fn, name=name, ty=ty_code, value=value, source=source_opt,
                          flags=flags_code, kw=kw_code, tags=tags)
        return T(code, "mod")

    # ------------------------------------------------------------ statements
    def comment_lines(self, a, b, done):
        out = []
        cm = self.src.comments()
        for ln in range(a, b + 1):
            if ln in cm and ln not in done:
                done.add(ln)
                if cm[ln]:
                    out.append("// %s" % cm[ln])
                else:
                    out.append("//")
        return out

    def block(self, blk, env, fnret, done, depth=0, start=None):
        lines = []
        env = dict(env)
        prev_end = blk.line if start is None else start
        for st in blk.stmts:
            lines += self.comment_lines(prev_end, st.line, done)
            lines.append("// L%d" % st.line)
            lines += self.stmt(st, env, fnret, done, depth)
            prev_end = st.endline
        lines += self.comment_lines(prev_end, blk.endline, done)
        return lines

    def stmt(self, st, env, fnret, done, depth):
        k = st.kind
        if k == "Local":
            if len(st.names) != 1 or len(st.exprs) != 1:
                self.fail("local-shape", st)
            name = st.names[0][0]
            t = self.ex(st.exprs[0], env)
            if t.multi:
                t = T(t.code, t.ty)
            rid = rust_ident(name)
            if t.ty in ("ifexpr", "orexpr"):
                self.fail("local-conditional", st)
            if t.ty in ("table", "unit", "nil"):
                self.fail("local-type", st, t.ty)
            if t.ty in STRINGY:
                # string locals are owned Strings so that later assignments can replace them
                code, ty = self.owned_string(t, st), "str"
            else:
                code, ty = t.code, t.ty
            out = ["let mut %s = %s;" % (rid, code)]
            env[name] = T(rid, ty)
            return out
        if k == "Assign":
            if len(st.targets) != 1 or len(st.exprs) != 1 or st.compound:
                self.fail("assign-shape", st)
            tgt = st.targets[0]
            if tgt.kind != "Name" or tgt.name not in env:
                self.fail("assign-target", st)
            cur = env[tgt.name]
            t = self.ex(st.exprs[0], env)
            if cur.ty == "Val":
                code = self.conv(t, "Val", st)
            elif cur.ty == "num":
                code = self.conv(t, "num", st)
            elif cur.ty == "name":
                code = self.conv(t, "name", st)
            elif cur.ty == "str" and cur.lit != "&str" and t.ty in STRINGY:
                code = self.owned_string(t, st)
            else:
                self.fail("assign-type", st, cur.ty)
            return ["%s = %s;" % (cur.code, code)]
        if k == "CallStat":
            c = st.call
            if c.kind == "MethodCall" and c.method == "NewMod":
                recv = self.ex(c.obj, env)
                if recv.ty != "modlist":
                    self.fail("NewMod-receiver", st, recv.ty)
                t = self.mod_call("NewMod", c.args, env, c, obj=recv.code)
                return [t.code + ";"]
            self.fail("call-statement", st, self.src.node_text(c)[:60])
        if k == "If":
            out = []
            for i, (cnd, body) in enumerate(st.clauses):
                cc = self.cond(cnd, env)
                head = "if %s {" % cc if i == 0 else "} else if %s {" % cc
                out.append(head)
                out += ["    " + ln for ln in self.block(body, env, fnret, done, depth + 1, start=cnd.line)]
            if st.orelse is not None:
                out.append("} else {")
                prev = st.clauses[-1][1].endline
                out += ["    " + ln for ln in self.block(st.orelse, env, fnret, done, depth + 1, start=prev)]
            out.append("}")
            out += self.comment_lines(st.line, st.endline, done)
            return out
        if k == "Return":
            if fnret == "unit":
                if st.exprs:
                    self.fail("return-value", st)
                return ["return;"]
            if len(st.exprs) > 1:
                self.fail("return-multi", st)
            t = self.ex(st.exprs[0], env) if st.exprs else T("None", "nil")
            return ["return %s;" % self.ret_value(t, fnret, st)]
        self.fail("stmt", st, k)

    def ret_value(self, t, fnret, n):
        if fnret == "optmods":
            return self.conv(t, "optmods", n)
        if fnret == "spec":
            return self.conv(t, "spec", n)
        self.fail("ret-kind", n, fnret)

    def fn_body(self, f, env, fnret, done):
        """Body of a closure returning `fnret`: a single `return e` becomes the expression;
        anything else becomes a block with `return`s and the implicit nil at the end."""
        body = f.body
        if len(body.stmts) == 1 and body.stmts[0].kind == "Return" and len(body.stmts[0].exprs) == 1:
            t = self.ex(body.stmts[0].exprs[0], env)
            if t.multi:
                self.fail("multi-return-expansion", body.stmts[0])
            return None, self.ret_value(t, fnret, body.stmts[0])
        lines = self.block(body, env, fnret, done)
        if not body.stmts or body.stmts[-1].kind != "Return":
            if fnret == "optmods":
                lines.append("None")
            else:
                self.fail("fallthrough-nil", f, fnret)
        return lines, None

    # -- closure parameter binding (ModParser lists)
    def bind_caps(self, f, listname, pattern):
        """Bind the closure's params to Caps (doc §6) per the list's call shape:
        specialModList: f(tonumber(cap1), cap1..cap5) (ModParser.lua:6662-6663)
        preFlagList:    f(cap1..cap5)                 (ModParser.lua:6680-6682)
        modTagList:     f(tonumber(cap1) or cap1, cap1..cap5) (ModParser.lua:6698-6702)."""
        try:
            caps = lpat.captures(pattern)
        except ValueError as e:
            self.fail("bad-pattern", f, str(e))
        if any(c == "pos" for c in caps):
            self.fail("position-capture", f)
        ncaps = len(caps)
        env = {}
        lines = []
        for k, (p, _ln) in enumerate(f.params, 1):
            if not _uses_name(f.body, p):
                continue
            rid = rust_ident(p)
            if listname == "preFlagList":
                ci = k - 1
                kind = "str"
            elif k == 1 and listname == "specialModList":
                kind = "num"
            elif k == 1 and listname == "modTagList":
                d = lpat.first_capture_digits(pattern)
                if d == "num":
                    kind = "num"
                elif d == "str":
                    kind, ci = "str", 0
                elif d == "none":
                    # `tagCap[1]:match("%d+")` indexes nil: Lua raises before calling the closure
                    self.fail("modtag-closure-without-capture", f)
                else:
                    self.fail("modtag-param1-num-or-string", f)
            else:
                kind, ci = "str", k - 2
            if kind == "num":
                lines.append("let %s = %s;" % (rid, self.shape["caps_num"]))
                env[p] = T(rid, "optnum")
            elif ci < ncaps:
                self.needs.add("ns")
                lines.append('let %s = ns(%s, "index local \'%s\'");' % (rid, self.S("caps_cap", i=ci), p))
                env[p] = T(rid, "str", lit="&str")
            else:
                env[p] = T("None", "nil")
        if f.vararg:
            self.fail("vararg-closure", f)
        return env, lines


def _lua_tonumber(s):
    t = s.strip(" \t\n\r\f\v")
    try:
        if t.lower().startswith(("0x", "-0x", "+0x")):
            return float(int(t, 16))
        v = float(t)
        if t.lower() in ("inf", "-inf", "+inf", "nan", "infinity", "-infinity"):
            return None
        return int(v) if v.is_integer() and "." not in t and "e" not in t.lower() else v
    except ValueError:
        return None
