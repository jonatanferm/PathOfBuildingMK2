"""ModParser pattern lists -> Rust `PatternList`s (calc doc §6, §8).

Lists: formList (72-158), modNameList (161-910), modFlagList (913-1099),
preFlagList (1102-1317), modTagList (1320-2009), specialModList (2130-5942).

Every entry becomes one `PatternEntry { pat, val, lua_line }` in Lua source order
(doc §6: "built once in source order"). Keys are emitted verbatim; the `^…$` wrapping
of specialModList keys (ModParser.lua:5949-5953) and the generated keystone/skill
entries (5943-5948, 6100-6150) belong to MP0. A duplicated key keeps the last value
(Lua table constructor semantics) and the overwritten entry is a `LUA-QUIRK` comment.

Entries lua2rs cannot translate are emitted as `unported!` placeholders preceded by
their Lua source, so the list still has the right count/order and compiles.
"""

from .common import LuaSource, Untranslatable, indent, rust_str, PIN_SHORT
from .translate import FILE_HELPERS, Translator

MODPARSER = "Modules/ModParser.lua"
LISTS = ("formList", "modNameList", "modFlagList", "preFlagList", "modTagList", "specialModList")


class Entry:
    __slots__ = ("key", "keynode", "val", "line", "endline", "cat", "ok", "reason", "code", "dup_of", "cdone", "notes")

    def __init__(self, key, keynode, val):
        self.key = key
        self.keynode = keynode
        self.val = val
        self.line = keynode.line
        self.endline = val.endline
        v = val
        if v.kind == "FunctionExpr":
            body = v.body.stmts
            if v.line == v.endline:
                self.cat = "fn-oneline"
            elif len(body) == 1 and body[0].kind == "Return":
                self.cat = "fn-singleret"
            else:
                self.cat = "fn-multi"
        else:
            self.cat = "static"
        self.ok = None
        self.reason = None
        self.code = None
        self.dup_of = None
        self.cdone = set()
        self.notes = []


def find_list(src, name):
    for st in src.chunk.stmts:
        if st.kind == "Local" and st.names and st.names[0][0] == name and st.exprs:
            t = st.exprs[0]
            if t.kind != "Table":
                raise SystemExit("%s is not a table constructor" % name)
            return st, t
    raise SystemExit("list %s not found in %s" % (name, src.path))


def list_entries(src, name):
    st, t = find_list(src, name)
    entries = []
    for fk, key, v in t.fields:
        if fk != "keyed" or key.kind != "String":
            raise SystemExit("%s:%d: unexpected field shape in %s" % (src.path, v.line, name))
        entries.append(Entry(key.value, key, v))
    last = {}
    for e in entries:
        last[e.key] = e
    for e in entries:
        if last[e.key] is not e:
            e.dup_of = last[e.key]
    return st, t, entries


class ListGen:
    def __init__(self, src, shape, flag_names=None):
        self.src = src
        self.shape = shape
        self.tr = Translator(src, shape, "modparser", flag_names)

    # -- single entry
    def entry_val(self, name, e):
        tr = self.tr
        v = e.val
        if name == "formList":
            if v.kind != "String":
                raise Untranslatable("form-value", v.kind)
            return rust_str(v.value)
        if name in ("modNameList", "modFlagList"):
            if name == "modNameList" and v.kind == "String":
                tr.needs.add("spec_names")
                return "%s(&[%s])" % (self.shape["spec_names_fn"], tr.name_lit(v.value))
            if v.kind != "Table":
                raise Untranslatable("spec-value", v.kind)
            return tr.spec(v, {})
        if name in ("preFlagList", "modTagList"):
            if v.kind == "Table":
                return tr.S("tag_spec", spec=tr.spec(v, {}))
            if v.kind == "FunctionExpr":
                return tr.S("tag_fn", closure=self.closure(name, e, "spec"))
            raise Untranslatable("tag-value", v.kind)
        if name == "specialModList":
            if v.kind == "Table":
                return tr.S("special_mods", mods=tr.mods_table(v, {}))
            if v.kind == "FunctionExpr":
                return tr.S("special_fn", closure=self.closure(name, e, "optmods"))
            raise Untranslatable("special-value", v.kind)
        raise SystemExit("unknown list %s" % name)

    def closure(self, name, e, fnret):
        tr = self.tr
        f = e.val
        env, binds = tr.bind_caps(f, name, e.key)
        done = set()
        lines, expr = tr.fn_body(f, env, fnret, done)
        e.cdone = done
        if expr is not None and not binds:
            body = expr
        else:
            inner = binds + (lines if lines is not None else [expr])
            body = "{\n" + "\n".join(indent(inner, 1)) + "\n}"
        return tr.S("closure", body=body)

    def placeholder(self, name, e):
        site = rust_str("%s:%d-%d" % (self.src.path, e.line, e.endline))
        unp = self.tr.S("unported", site=site)
        if name in ("preFlagList", "modTagList"):
            return self.tr.S("tag_fn", closure=self.tr.S("closure", body=unp))
        if name == "specialModList":
            return self.tr.S("special_fn", closure=self.tr.S("closure", body=unp))
        return unp

    def translate_entry(self, name, e):
        saved = set(self.tr.needs)
        self.tr.notes = []
        try:
            val = self.entry_val(name, e)
            e.ok = True
            e.notes = list(self.tr.notes)
        except Untranslatable as x:
            self.tr.needs = saved
            e.ok = False
            e.reason = x.reason
            e.code = None
            val = self.placeholder(name, e)
            e.code = (x, val)
            return
        e.code = val

    # -- file emission
    def emit_entry(self, name, e, done):
        cm = self.src.comments()
        out = []
        for ln in range(e.line, e.endline + 1):
            if ln in cm and ln not in done and not (e.ok and ln in e.cdone):
                done.add(ln)
                out.append("// %s" % cm[ln] if cm[ln] else "//")
        pat = rust_str(e.key)
        if e.dup_of is not None:
            out.append("// LUA-QUIRK: duplicate key %s (L%d) is overwritten by L%d; the Lua table keeps the last value"
                       % (pat, e.line, e.dup_of.line))
            return out
        if e.ok:
            val = e.code
            out += ["// " + nt for nt in e.notes]
        else:
            x, val = e.code
            out.append(self.tr.S("unported_macro_note", reason=x))
            for ln in self.src.text(e.line, e.endline):
                out.append("// | " + ln.rstrip().replace("\t", "    "))
        entry = self.tr.S("pattern_entry", pat=pat, val=val, line=e.line)
        out += ("v.push(%s);" % entry).split("\n")
        return out

    def generate(self, name, rng=None, fn_name=None, header_extra=None):
        """Rust source for list `name` (optionally only entries whose key line is in rng)."""
        st, t, entries = list_entries(self.src, name)
        whole = rng is None or (rng[0] <= st.line and rng[1] >= t.endline)
        if whole:
            a, b = st.line, t.endline
        else:
            a, b = rng
        sel = [e for e in entries if a <= e.line <= b]
        if not sel:
            raise SystemExit("no %s entries in %d-%d" % (name, a, b))
        if not whole:
            b = max(b, sel[-1].endline)
        for e in sel:
            if e.dup_of is None:
                self.translate_entry(name, e)
        ty = self.shape["list_types"][name]
        bs = int(self.shape["block_size"])
        blocks = [sel[i:i + bs] for i in range(0, len(sel), bs)]
        body = []
        block_names = []
        start = a
        done = set()
        cm = self.src.comments()
        for bi, blk in enumerate(blocks):
            end = b if bi == len(blocks) - 1 else blk[-1].endline
            fname = "%s_%d" % (name, blk[0].line)
            block_names.append(fname)
            body.append("/// " + self.src.marker(start, end))
            body.append(self.tr.S("block_fn", name=fname, ty=ty))
            inner = []
            prev = start
            for e in blk:
                for ln in range(prev, e.line):
                    if ln in cm and ln not in done:
                        done.add(ln)
                        inner.append("// %s" % cm[ln] if cm[ln] else "//")
                inner += self.emit_entry(name, e, done)
                prev = e.endline + 1
            for ln in range(prev, end + 1):
                if ln in cm and ln not in done:
                    done.add(ln)
                    inner.append("// %s" % cm[ln] if cm[ln] else "//")
            body += indent(inner, 1)
            body.append("}")
            body.append("")
            start = end + 1
        calls = indent(["%s(&mut v);" % bn for bn in block_names], 1)
        n_entries = sum(1 for e in sel if e.dup_of is None)
        if whole and fn_name is None:
            decl = self.tr.S("list_decl", name=name, ty=ty, n=n_entries, calls="\n".join(calls))
            body += decl.split("\n")
        else:
            fn = fn_name or "push_%s_%d_%d" % (name, a, b)
            body.append("/// Pushes the %s entries of %s:%d-%d (%d entries) in source order." % (name, self.src.path, a, b, n_entries))
            body.append(self.tr.S("range_push_fn", name=fn, ty=ty))
            body += ["    %s(&mut v);" % bn for bn in block_names]
            body.append("}")
        head = ["//! @pob-file %s @%s" % (self.src.path, PIN_SHORT),
                "//! GENERATED by scripts/port/lua2rs (unit T1) from %s:%d-%d (`%s`)." % (self.src.path, a, b, name)]
        if header_extra:
            head += ["//! " + h for h in header_extra]
        head += self.shape["prelude"]["modparser"]
        head.append("")
        for h in sorted(self.tr.needs):
            head += FILE_HELPERS[h]
            head.append("")
        return "\n".join(head + body) + "\n", sel


def stats(entries):
    """Coverage numbers for a list of translated entries (duplicates excluded)."""
    out = {}
    live = [e for e in entries if e.dup_of is None]
    for cat in ("static", "fn-oneline", "fn-singleret", "fn-multi"):
        es = [e for e in live if e.cat == cat]
        out[cat] = {"total": len(es), "ok": sum(1 for e in es if e.ok)}
    out["all"] = {"total": len(live), "ok": sum(1 for e in live if e.ok)}
    so = [e for e in live if e.cat in ("static", "fn-oneline")]
    out["static+oneline"] = {"total": len(so), "ok": sum(1 for e in so if e.ok)}
    reasons = {}
    for e in live:
        if not e.ok:
            reasons[e.reason] = reasons.get(e.reason, 0) + 1
    out["fail_reasons"] = dict(sorted(reasons.items(), key=lambda kv: -kv[1]))
    out["duplicates"] = [e.line for e in entries if e.dup_of is not None]
    return out
