"""ConfigOptions `apply = function(val, modList, enemyModList[, build])` closures -> Rust fns
plus a `ROWS` registry table (calc doc §8.1: `ConfigApply`, `FnRow`).

Ids follow the dump (AMENDMENTS A2, closures.json): `ConfigOptions[<i>].apply`, where i is
the option's 1-based index in the table the module returns; the hash is the sha256 of the
function's source lines (pob-dump FORMAT.md "Functions"). The shared
`mapAffixDropDownFunction` (ConfigOptions.lua:117-130, applied by 8 options) is not an
inline closure and is reported separately.

`val` stays the dynamic `&Val` (§5.1): comparisons and arithmetic use Lua's rules
(`val == "X"` does not coerce, arithmetic coerces numeric strings via `Val::num`), and a
mod value stores whatever `val` holds (`val_value`).
"""

from .common import LuaSource, Untranslatable, indent, rust_ident, rust_str, PIN_SHORT
from .translate import FILE_HELPERS, T, Translator, _assigned_names

CONFIG = "Modules/ConfigOptions.lua"


class Apply:
    __slots__ = ("index", "var", "func", "line", "endline", "ok", "reason", "rust_name", "lines", "id", "hash", "shared")

    def __init__(self, index, var, func, shared=None):
        self.index = index
        self.var = var
        self.func = func
        self.shared = shared
        self.line = func.line if func is not None else None
        self.endline = func.endline if func is not None else None
        self.ok = None
        self.reason = None
        self.rust_name = None
        self.lines = None
        self.id = "ConfigOptions[%d].apply" % index
        self.hash = None


def list_applies(src):
    ret = src.chunk.stmts[-1]
    if ret.kind != "Return" or len(ret.exprs) != 1 or ret.exprs[0].kind != "Table":
        raise SystemExit("%s does not end in `return { ... }`" % src.path)
    out = []
    idx = 0
    for fk, key, opt in ret.exprs[0].fields:
        if fk != "pos":
            continue
        idx += 1
        if opt.kind != "Table":
            continue
        d = {k.value: v for f, k, v in opt.fields if f == "named"}
        if "apply" not in d:
            continue
        var = d["var"].value if "var" in d and d["var"].kind == "String" else None
        a = d["apply"]
        if a.kind == "FunctionExpr":
            out.append(Apply(idx, var, a))
        else:
            out.append(Apply(idx, var, None, shared=src.node_text(a)))
    return out


class ConfigGen:
    def __init__(self, src, shape, flag_names=None):
        self.src = src
        self.shape = shape
        self.tr = Translator(src, shape, "config", flag_names)

    def translate(self, ap, names_used):
        tr = self.tr
        f = ap.func
        base = rust_ident(ap.var) if ap.var else "apply_%d" % ap.index
        nm = base
        if nm in names_used:
            nm = "%s_%d" % (base, ap.index)
        names_used.add(nm)
        ap.rust_name = nm
        ap.hash = self.src.fn_hash(f.line, f.endline)
        params = [p for p, _ in f.params]
        if f.vararg or len(params) > 4:
            pnames = None
        else:
            pnames = params
        roles = ("val", "modList", "enemyModList", "build")
        rust_params = {}
        env = {}
        for i, role in enumerate(roles):
            if pnames is not None and i < len(pnames):
                rid = rust_ident(pnames[i])
                rust_params[role] = rid
                env[pnames[i]] = T(rid, ("Val", "modlist", "modlist", "build")[i])
            else:
                rust_params[role] = "_" + role
        sig = tr.S("config_fn", name=nm, **rust_params)
        saved = set(tr.needs)
        tr.notes = []
        try:
            if pnames is None:
                raise Untranslatable("apply-params")
            pre = []
            for p in sorted(_assigned_names(f.body)):
                if p in env and env[p].ty == "Val" and p == pnames[0]:
                    pre.append("let mut %s: Val = %s.clone(); // Lua reassigns the parameter" % (env[p].code, env[p].code))
                elif p in env:
                    raise Untranslatable("param-reassigned", p)
            done = set()
            head_comments = tr.comment_lines(f.line, max(f.line, f.body.line - 1) if f.body.stmts else f.line, done)
            body = tr.block(f.body, env, "unit", done)
            ap.ok = True
            ap.lines = ["// " + nt for nt in tr.notes] + head_comments + pre + body
        except Untranslatable as x:
            tr.needs = saved
            ap.ok = False
            ap.reason = x.reason
            lua = ["// | " + ln.rstrip().replace("\t", "    ") for ln in self.src.text(f.line, f.endline)]
            site = rust_str("%s:%d-%d" % (self.src.path, f.line, f.endline))
            ap.lines = [tr.S("unported_macro_note", reason=x)] + lua + [tr.S("unported", site=site)]
        out = ["/// " + self.src.marker(f.line, f.endline)]
        if ap.var:
            out.append("/// var = %s" % rust_str(ap.var))
        out.append(sig)
        out += indent(ap.lines, 1)
        out.append("}")
        return out

    def generate(self, rng=None, header_extra=None):
        applies = list_applies(self.src)
        inline = [a for a in applies if a.func is not None]
        if rng is not None:
            inline = [a for a in inline if rng[0] <= a.line <= rng[1]]
        names_used = set()
        body = []
        for ap in inline:
            body += self.translate(ap, names_used)
            body.append("")
        body.append("/// Registry rows (calc doc §8.1); ids and hashes as in data/dump/closures.json.")
        body.append(self.tr.S("config_rows_decl"))
        for ap in inline:
            body.append("    " + self.tr.S("config_row", id=rust_str(ap.id), hash=rust_str(ap.hash), name=ap.rust_name) + ",")
        body.append("];")
        a = rng[0] if rng else 1
        b = rng[1] if rng else len(self.src.lines)
        head = ["//! @pob-file %s @%s" % (self.src.path, PIN_SHORT),
                "//! GENERATED by scripts/port/lua2rs (unit T1) from the `apply` closures of %s:%d-%d." % (self.src.path, a, b)]
        if header_extra:
            head += ["//! " + h for h in header_extra]
        head += self.shape["prelude"]["config"]
        head.append("")
        for h in sorted(self.tr.needs):
            head += FILE_HELPERS[h]
            head.append("")
        return "\n".join(head + body) + "\n", inline, [x for x in applies if x.func is None]


def stats(inline, shared):
    reasons = {}
    for a in inline:
        if not a.ok:
            reasons[a.reason] = reasons.get(a.reason, 0) + 1
    return {
        "inline": {"total": len(inline), "ok": sum(1 for a in inline if a.ok)},
        "shared_fn_options": len(shared),
        "fail_reasons": dict(sorted(reasons.items(), key=lambda kv: -kv[1])),
    }
