#!/usr/bin/env python3
"""Inventory of every `pairs()` site in the calc scope, with its key class (calc doc §3.4, D8).

Key classes:
  int       integer keys. LuaJIT's order is deterministic but is not insertion order;
            where the order is observable the port uses `pob_lua::ljtab::LjIntMap`.
            Shape `list` marks a sequence (1..n, built with t_insert or a positional
            constructor): LuaJIT walks its array part in index order.
  string    string keys. LuaJIT's order is random per process (seeded string hash);
            the port uses insertion order and the oracle's nondet runs (§9.5).
  identity  table/function keys (address hashing, random per process); the port uses
            insertion order (`PtrMap`/`PtrSet`).
  mixed     both integer and string keys (e.g. a table with an array part and named
            fields); the int part follows LuaJIT, the string part insertion order.
  generic   a helper that iterates whatever table it is given (copyTable, wipeTable,
            ...): the order is the argument's. Copying a table with pairs can change
            LuaJIT's integer-key layout of the copy (insertion order matters for colliding
            keys), so an int-keyed copy must replay the same insert sequence.

Each site's class is inferred from (in order): a table-constructor argument; how the
loop's key variable is used in the body (string methods, `..`, comparisons with
literals, field access, arithmetic, indexing a known map); the iterated table's name
(`nodes`, `allocNodes`, `mods`, `conditions`, ...); and the writes into a local table
in the same function (`t[k] = v`, `t_insert(t, v)`). Sites the heuristics cannot decide
are classified by hand in OVERRIDES (each with its reason); `--check` fails on any
unclassified site and on any stale override.

Also listed: generic-for loops over `next` (`for k, v in next, t`), which iterate exactly
like pairs. Plain `next(t)` calls (emptiness tests) are counted, not listed.

Usage:
  pairs_inventory.py [--out docs/port/pairs_inventory.md] [--scope FILE]
  pairs_inventory.py --check      exit 1 if any site is unclassified or an override is stale
  pairs_inventory.py --json
"""

import argparse
import json
import os
import sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))

import calcscope  # noqa: E402
import luaast  # noqa: E402
import pobpaths  # noqa: E402

DEFAULT_OUT = os.path.join(pobpaths.docs_port(), "pairs_inventory.md")

STRING_METHODS = frozenset("match sub find gsub gmatch lower upper len rep byte format reverse".split())
STRING_FUNCS = frozenset("s_format string.format s_match string.match s_gsub string.gsub s_find string.find "
                         "s_lower string.lower s_upper string.upper s_sub string.sub string.len "
                         "escapeGGGString".split())
ARITH = frozenset("+ - * / % ^".split())
CMP = frozenset("< > <= >= == ~=".split())

# Last path component of an iterated table -> (class, shape). Calc doc §3.4 names the
# node-id maps; the rest are PoB's well-known maps.
TABLE_NAMES = {
    # node id maps (integer node ids)
    "nodes": ("int", "map"), "allocNodes": ("int", "map"), "grantedPassives": ("int", "map"),
    "extraRadiusNodeList": ("int", "map"), "nodesInRadius": ("int", "map"), "allocExtendedNodes": ("int", "map"),
    "hashOverrides": ("int", "map"), "masterySelections": ("int", "map"), "jewels": ("int", "map"),
    "clusterNodeMap": ("string", "map"), "legacyClusterNodeMap": ("int", "map"),
    "addNodes": ("identity", "set"), "depends": ("int", "list"), "path": ("int", "list"),
    "linked": ("int", "list"), "intuitiveLeapLikesAffecting": ("int", "map"),
    "sockets": ("int", "map"), "keystoneMap": ("string", "map"), "notableMap": ("string", "map"),
    # mod stores and calc tables (string keys)
    "mods": ("string", "map"), "conditions": ("string", "map"), "multipliers": ("string", "map"),
    "input": ("string", "map"), "placeholder": ("string", "map"), "configInput": ("string", "map"),
    "output": ("string", "map"), "skills": ("string", "map"), "gems": ("string", "map"),
    "itemBases": ("string", "map"), "statMap": ("string", "map"), "weaponTypes": ("string", "map"),
    "slots": ("string", "map"), "varControls": ("string", "map"), "affixes": ("string", "map"),
    "classes": ("int", "map"), "alternate_ascendancies": ("int", "list"),
    "Item": ("string", "map"), "ItemExclusive": ("string", "map"),
    "buffs": ("string", "map"), "debuffs": ("string", "map"), "minionBuffs": ("string", "map"),
    "guards": ("string", "map"), "souls": ("int", "list"),
    "activeSkillList": ("int", "list"), "socketGroupList": ("int", "list"), "effectList": ("int", "list"),
    "levels": ("int", "list"), "varList": ("int", "list"), "skillNameList": ("int", "list"),
    "skillTypeList": ("int", "list"), "monsterTagList": ("int", "list"), "variantList": ("int", "map"),
    "versionList": ("int", "map"), "variantGroupList": ("int", "map"),
}

# Map names whose integer/string/identity nature identifies a key used to index them.
INDEXED_BY = {
    "nodes": "int", "allocNodes": "int", "grantedPassives": "int", "nodesInRadius": "int",
    "hashOverrides": "int", "masterySelections": "int", "allocExtendedNodes": "int",
    "skills": "string", "gems": "string", "itemBases": "string", "mods": "string",
    "conditions": "string", "multipliers": "string", "slots": "string",
}

# Hand-classified sites: (file, line, iterated expression) -> (class, shape, reason).
OVERRIDES = {}


def _register_overrides():
    def o(path, line, expr, cls, shape, why):
        OVERRIDES[(path, line, expr)] = (cls, shape, why)
    # Filled from review of the sites the heuristics leave undecided (see bottom of file).
    for row in _OVERRIDE_ROWS:
        o(*row)


class Site:
    def __init__(self, path, line, func, expr, keyvar, via):
        self.path = path
        self.line = line
        self.func = func
        self.expr = expr
        self.keyvar = keyvar
        self.via = via          # 'pairs' | 'next-for'
        self.cls = None
        self.shape = ""
        self.evidence = []
        self.override = False


def _dotted(e):
    """Dotted path of a Name/Index chain, or None."""
    parts = []
    while True:
        if e.kind == "Name":
            parts.append(e.name)
            break
        if e.kind == "Index":
            if e.key.kind == "String":
                parts.append(e.key.value)
            elif e.key.kind == "Number":
                parts.append("[%s]" % e.key.value)
            else:
                parts.append("[]")
            e = e.obj
            continue
        if e.kind == "Paren":
            e = e.expr
            continue
        return None
    return ".".join(reversed(parts))


def _last_name(e):
    while e.kind == "Paren":
        e = e.expr
    if e.kind == "Name":
        return e.name
    if e.kind == "Index":
        if e.key.kind == "String":
            return e.key.value
        return _last_name(e.obj)  # nodesInRadius[radius] -> nodesInRadius
    if e.kind == "BinOp" and e.op == "or":
        return _last_name(e.left)
    return None


def _call_name(c):
    if c.kind == "Call":
        return _dotted(c.func)
    return None


def expr_key_class(e):
    """Class of a value used as a key, from the expression alone (or None)."""
    k = e.kind
    if k == "String":
        return "string"
    if k == "Number":
        return "int"
    if k == "BinOp":
        if e.op == "..":
            return "string"
        if e.op in ARITH:
            return "int"
    if k == "UnOp" and e.op == "-":
        return "int"
    if k == "Call":
        nm = _call_name(e)
        if nm in STRING_FUNCS:
            return "string"
        if nm in ("tonumber", "m_floor", "math.floor", "m_ceil", "math.ceil", "round"):
            return "int"
    if k == "MethodCall" and e.method in STRING_METHODS:
        return "string"
    if k == "Index" and e.key.kind == "String" and e.key.value in ("id", "nodeId") and \
            _last_name(e.obj) in ("node", "otherNode", "subNode", "newNode", "nodeInRadius", "notable"):
        return "int"
    if k == "Name":
        d = getattr(e, "decl", None)
        if d is not None and d.kind == "for" and d.node is not None and d.node.kind == "NumFor":
            return "int"
    return None


def _parents(root):
    par = {}
    for n in luaast.walk(root):
        for c in luaast.children(n):
            par[id(c)] = n
    return par


def keyvar_usage(decl, par):
    """Evidence about a loop key variable from its uses."""
    ev = []
    for r in decl.refs:
        n = r.node
        p = par.get(id(n))
        if p is None:
            continue
        k = p.kind
        if k == "Index" and p.obj is n:
            ev.append(("identity", "%s.field" % decl.name))
        elif k == "Index" and p.key is n:
            nm = _last_name(p.obj)
            if nm in INDEXED_BY:
                ev.append((INDEXED_BY[nm], "indexes %s" % nm))
        elif k == "MethodCall" and p.obj is n:
            if p.method in STRING_METHODS:
                ev.append(("string", "%s:%s()" % (decl.name, p.method)))
            else:
                ev.append(("identity", "%s:%s()" % (decl.name, p.method)))
        elif k == "BinOp":
            other = p.right if p.left is n else p.left
            if p.op == "..":
                ev.append(("string?", "%s .. x" % decl.name))
            elif p.op in ARITH:
                ev.append(("int", "arithmetic on %s" % decl.name))
            elif p.op in CMP:
                if other.kind == "String":
                    ev.append(("string", '%s %s "%s"' % (decl.name, p.op, other.value)))
                elif other.kind == "Number":
                    ev.append(("int", "%s %s %s" % (decl.name, p.op, other.value)))
        elif k == "UnOp" and p.op == "-":
            ev.append(("int", "-%s" % decl.name))
        elif k == "Call" and n in p.args:
            nm = _call_name(p)
            if nm in STRING_FUNCS:
                ev.append(("string?", "%s(%s)" % (nm, decl.name)))
            elif nm == "type":
                pp = par.get(id(p))
                if pp is not None and pp.kind == "BinOp" and pp.op in ("==", "~="):
                    other = pp.right if pp.left is p else pp.left
                    if other.kind == "String":
                        ev.append(({"number": "int", "string": "string", "table": "identity"}.get(other.value, "mixed"),
                                   'type(%s) == "%s"' % (decl.name, other.value)))
            elif nm in ("tostring",):
                pass
        elif k == "MethodCall" and n in p.args and p.method in ("Sum", "More", "Flag", "Override", "List",
                                                                "GetCondition", "GetMultiplier", "NewMod",
                                                                "Max", "Min", "Tabulate", "HasMod"):
            # PoB mod names are strings
            if p.args and p.args[0] is not n or p.method in ("GetCondition", "GetMultiplier"):
                ev.append(("string", "mod name in :%s()" % p.method))
    return ev


def local_table_writes(decl, par):
    """Evidence from writes into a local table: t[k] = v, t_insert(t, v)."""
    ev = []
    for r in decl.refs:
        n = r.node
        p = par.get(id(n))
        if p is None:
            continue
        if p.kind == "Index" and p.obj is n:
            pp = par.get(id(p))
            if pp is not None and pp.kind == "Assign" and p in pp.targets:
                if p.dot:
                    ev.append(("string", "%s.%s = ..." % (decl.name, p.key.value)))
                else:
                    c = expr_key_class(p.key)
                    if c:
                        ev.append((c, "%s[%s] = ..." % (decl.name, c)))
                    elif p.key.kind == "Name" and getattr(p.key, "decl", None) is not None:
                        kd = p.key.decl
                        if kd.init is not None and kd.init.kind == "Table":
                            ev.append(("identity", "%s[<table>] = ..." % decl.name))
        elif p.kind == "Call" and p.args and p.args[0] is n and _call_name(p) in ("t_insert", "table.insert"):
            ev.append(("int", "t_insert(%s, ...)" % decl.name))
    if decl.init is not None and decl.init.kind == "Table":
        ev.extend(constructor_evidence(decl.init, decl.name))
    return ev


def constructor_evidence(t, label="{...}"):
    ev = []
    for kind, key, _ in t.fields:
        if kind == "pos":
            ev.append(("int", "%s positional" % label))
        elif kind == "named":
            ev.append(("string", "%s named field" % label))
        else:
            c = expr_key_class(key)
            if c:
                ev.append((c, "%s [%s] key" % (label, c)))
            elif key.kind == "Name" and getattr(key, "decl", None) is not None and key.decl.init is not None \
                    and key.decl.init.kind == "Table":
                ev.append(("identity", "%s [<table>] key" % label))
    return ev


def decide(ev):
    strong = [c for c, _ in ev if not c.endswith("?")]
    weak = [c.rstrip("?") for c, _ in ev if c.endswith("?")]
    s = set(strong)
    if len(s) == 1:
        return s.pop()
    if len(s) > 1:
        if s == {"int", "string"}:
            return "mixed"
        return None
    w = set(weak)
    if len(w) == 1:
        return w.pop()
    return None


def classify(site, call, forstmt, par, chunk):
    arg = call.args[0] if call.args else None
    ev = []
    shape = ""
    if arg is not None and arg.kind == "Table":
        ev = constructor_evidence(arg)
        if all(k == "pos" for k, _, _ in arg.fields):
            shape = "list"
    else:
        if forstmt is not None and forstmt.decls:
            ev.extend(keyvar_usage(forstmt.decls[0], par))
        if arg is not None:
            nm = _last_name(arg)
            if nm in TABLE_NAMES:
                c, shp = TABLE_NAMES[nm]
                ev.append((c, "table name %s" % nm))
                shape = shp
            base = arg
            while base.kind == "Paren":
                base = base.expr
            if base.kind == "BinOp" and base.op == "or":
                base = base.left
            if base.kind == "Name" and getattr(base, "decl", None) is not None:
                tev = local_table_writes(base.decl, par)
                ev.extend(tev)
                if tev and all(c == "int" and "t_insert" in why or "positional" in why for c, why in tev):
                    shape = "list"
            elif base.kind in ("MethodCall",) and base.method == "List":
                ev.append(("int", ":List() returns a list"))
                shape = "list"
    site.evidence = ev
    site.cls = decide(ev)
    site.shape = shape


def find_sites(path, chunk, res, scope):
    par = _parents(chunk)
    sites = []
    next_calls = 0
    for n in luaast.walk(chunk):
        if n.kind == "GenFor" and n.exprs:
            e0 = n.exprs[0]
            if e0.kind == "Name" and e0.name == "next" and getattr(e0, "decl", None) is None and len(n.exprs) >= 2:
                if not scope.contains(path, n.line):
                    continue
                s = Site(path, n.line, luaast.enclosing_function_name(_func_of(n, par)),
                         luaast.source_text(chunk, n.exprs[1]), n.names[0][0], "next-for")
                fake = luaast.Node("Call", n.line, func=e0, args=[n.exprs[1]])
                classify(s, fake, n, par, chunk)
                sites.append(s)
        if n.kind != "Call" or n.func.kind != "Name":
            continue
        if not scope.contains(path, n.line):
            continue
        fname = n.func.name
        d = getattr(n.func, "decl", None)
        if d is not None:
            # `local pairs = pairs` aliases count as pairs
            if not (d.init is not None and d.init.kind == "Name" and d.init.name == fname and d.init.decl is None):
                continue
        if fname == "next":
            next_calls += 1
            continue
        if fname != "pairs":
            continue
        p = par.get(id(n))
        forstmt = p if (p is not None and p.kind == "GenFor" and p.exprs and p.exprs[0] is n) else None
        expr = luaast.source_text(chunk, n.args[0]) if n.args else ""
        s = Site(path, n.line, luaast.enclosing_function_name(_func_of(n, par)), expr,
                 forstmt.names[0][0] if forstmt else None, "pairs")
        classify(s, n, forstmt, par, chunk)
        sites.append(s)
    return sites, next_calls


def _func_of(n, par):
    p = par.get(id(n))
    while p is not None and p.kind != "FunctionExpr":
        p = par.get(id(p))
    return p


def inventory(pob, scope):
    all_sites = []
    next_calls = 0
    for path in scope.files():
        full = os.path.normpath(os.path.join(pob, "src", path))
        chunk = luaast.parse_file(full)
        res = luaast.resolve(chunk)
        s, nc = find_sites(path, chunk, res, scope)
        all_sites.extend(s)
        next_calls += nc
    used = set()
    for s in all_sites:
        key = (s.path, s.line, s.expr)
        if key in OVERRIDES:
            cls, shape, why = OVERRIDES[key]
            used.add(key)
            s.evidence = [(cls, "by hand: " + why)] + s.evidence
            s.cls = cls
            s.shape = shape or s.shape
            s.override = True
    in_scope = set(scope.files())  # overrides for files outside the scope (e.g. no closures.json) are not stale
    stale = sorted(k for k in set(OVERRIDES) - used if k[0] in in_scope)
    all_sites.sort(key=lambda s: (s.path, s.line, s.expr))
    return all_sites, next_calls, stale


def render_md(sites, next_calls, scope_src):
    from collections import Counter, OrderedDict
    cnt = Counter(s.cls or "UNCLASSIFIED" for s in sites)
    out = []
    out.append("# pairs() inventory (calc scope)")
    out.append("")
    out.append("Generated by `scripts/port/pairs_inventory.py` at the pin %s; do not edit by hand." % pobpaths.PIN_SHORT)
    out.append("Produced by P4, frozen by U0 (calc doc §3.4). Scope: %s." % scope_src)
    out.append("")
    out.append("Key classes (calc doc §3.4, D8): **int** follows LuaJIT's integer-key order (deterministic, not "
               "insertion order; `LjIntMap` where the order is observable; shape `list` = a 1..n sequence, walked in "
               "index order); **string** is random per PoB process (insertion order in Rust, oracle nondet runs); "
               "**identity** (table keys) is random per process (`PtrMap`/`PtrSet`, insertion order); **generic** helpers (copyTable, wipeTable, ...) iterate whatever table they are given; **mixed** has "
               "both an integer and a string part.")
    out.append("")
    out.append("Sites: %d (%s). `for k, v in next, t` loops are included; plain `next(t)` calls (%d) are not." % (
        len(sites), ", ".join("%s %d" % (k, v) for k, v in sorted(cnt.items())), next_calls))
    out.append("")
    out.append("## Integer-keyed maps (LjIntMap candidates)")
    out.append("")
    out.append("Sites of class **int** that are not plain sequences (shape `map`, or unknown shape). "
               "Their LuaJIT order is not insertion order; wherever the loop's effect depends on the "
               "order, the port uses `pob_lua::ljtab::LjIntMap`.")
    out.append("")
    out.append("| Site | Iterated | Evidence |")
    out.append("|---|---|---|")
    for s in sites:
        if s.cls == "int" and s.shape != "list":
            out.append("| %s:%d | `%s` | %s |" % (s.path, s.line, s.expr.replace("|", "\\|"),
                                                 "; ".join(w for _, w in s.evidence[:2]).replace("|", "\\|")))
    out.append("")
    by_file = OrderedDict()
    for s in sites:
        by_file.setdefault(s.path, []).append(s)
    for path, ss in by_file.items():
        out.append("## %s" % path)
        out.append("")
        out.append("| Line | Function | Iterated | Key | Class | Shape | Evidence |")
        out.append("|---:|---|---|---|---|---|---|")
        for s in ss:
            ev = "; ".join(why for _, why in s.evidence[:3])
            expr = s.expr if len(s.expr) <= 80 else s.expr[:77] + "..."
            via = "" if s.via == "pairs" else " (next)"
            out.append("| %d | `%s` | `%s`%s | %s | **%s** | %s | %s |" % (
                s.line, s.func, expr.replace("|", "\\|"), via, ("`%s`" % s.keyvar) if s.keyvar else "",
                s.cls or "UNCLASSIFIED", s.shape, ev.replace("|", "\\|")))
        out.append("")
    return "\n".join(out)


def main(argv=None):
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--pob")
    ap.add_argument("--scope")
    ap.add_argument("--out", help="write the markdown inventory to this path")
    ap.add_argument("--write", action="store_true", help="write docs/port/pairs_inventory.md")
    ap.add_argument("--check", action="store_true")
    ap.add_argument("--json", action="store_true")
    ap.add_argument("--unclassified", action="store_true", help="print only unclassified sites with evidence")
    args = ap.parse_args(argv)
    if args.write and not args.out:
        args.out = DEFAULT_OUT
    if not OVERRIDES:
        _register_overrides()
    pob = pobpaths.find_pob(args.pob)
    scope_path = args.scope or os.path.join(pobpaths.docs_port(), "scope.toml")
    scope = calcscope.load_scope(scope_path, calcscope.default_closures_json(pobpaths.main_checkout()),
                                 required=bool(args.scope))
    sites, next_calls, stale = inventory(pob, scope)
    unclassified = [s for s in sites if s.cls is None]
    if args.unclassified:
        for s in unclassified:
            print("%s:%d\t%s\t%s\tkey=%s\t%s" % (s.path, s.line, s.func, s.expr, s.keyvar, s.evidence))
        print("%d unclassified of %d" % (len(unclassified), len(sites)))
        return 0
    if args.json:
        json.dump([{"file": s.path, "line": s.line, "function": s.func, "expr": s.expr, "key": s.keyvar,
                    "via": s.via, "class": s.cls, "shape": s.shape, "override": s.override,
                    "evidence": [w for _, w in s.evidence]} for s in sites], sys.stdout, indent=1)
        print()
        return 0
    if args.check:
        rc = 0
        for s in unclassified:
            print("FAIL unclassified: %s:%d %s (%s)" % (s.path, s.line, s.expr, s.evidence))
            rc = 1
        for k in stale:
            print("FAIL stale override: %s:%d %s" % k)
            rc = 1
        out = args.out or DEFAULT_OUT
        if os.path.exists(out):
            with open(out) as fh:
                if fh.read() != render_md(sites, next_calls, scope.source) + "\n":
                    print("FAIL %s is out of date; regenerate with --write" % os.path.relpath(out))
                    rc = 1
        if rc == 0:
            print("ok: %d pairs sites, all classified (%d by hand)" % (len(sites), sum(s.override for s in sites)))
        return rc
    md = render_md(sites, next_calls, scope.source)
    if args.out:
        with open(args.out, "w") as fh:
            fh.write(md + "\n")
        print("wrote %s: %d sites, %d unclassified" % (args.out, len(sites), len(unclassified)))
    else:
        print(md)
    return 1 if unclassified else 0


# (file, line, iterated expression, class, shape, reason)
_OVERRIDE_ROWS = [
    # Classes/CalcsTab.lua
    ("Classes/CalcsTab.lua", 594, "distanceMap", "int", "map", "distanceMap[dist] path distances (CalcsTab.lua:585); sorted after"),
    ("Classes/CalcsTab.lua", 770, "state", "string", "map", "undo state is a copy of self.input (config var names)"),
    # Classes/ConfigTab.lua
    ("Classes/ConfigTab.lua", 1066, "section.varControlList", "int", "list", "built with t_insert (ConfigTab.lua:264ff)"),
    # Classes/Item.lua
    ("Classes/Item.lua", 299, "self.explicitModLines", "int", "list", "mod line lists are built with t_insert"),
    ("Classes/Item.lua", 301, "self.enchantModLines", "int", "list", "mod line lists are built with t_insert"),
    ("Classes/Item.lua", 302, "self.scourgeModLines", "int", "list", "mod line lists are built with t_insert"),
    ("Classes/Item.lua", 303, "self.implicitModLines", "int", "list", "mod line lists are built with t_insert"),
    ("Classes/Item.lua", 304, "self.crucibleModLines", "int", "list", "mod line lists are built with t_insert"),
    ("Classes/Item.lua", 377, "self.variantGroupSelections", "int", "map", "keyed by variant group id (Item.lua:355)"),
    ("Classes/Item.lua", 696, "data.jewelRadius", "int", "list", "data.jewelRadius is a positional list"),
    ("Classes/Item.lua", 1553, "self.mutatedLines", "string", "map", "data.foulbornMap entry: mod id -> mod id (Item.lua:1028)"),
    ("Classes/Item.lua", 1951, "self.baseLines or { }", "string", "map", "self.baseLines[baseName] (Item.lua:1005)"),
    ("Classes/Item.lua", 1981, "self.baseLines or { }", "string", "map", "self.baseLines[baseName] (Item.lua:1005)"),
    ("Classes/Item.lua", 2238, "tag", "string", "map", "a mod tag table (type/var/... fields)"),
    ("Classes/Item.lua", 2267, "groupCounts", "int", "map", "groupCounts[group], socket group numbers (Item.lua:2261)"),
    # Classes/ModStore.lua
    ("Classes/ModStore.lua", 722, "items", "string", "map", "slot name -> item (ModStore.lua:694-711)"),
    ("Classes/ModStore.lua", 727, "items", "string", "map", "slot name -> item (ModStore.lua:694-711)"),
    ("Classes/ModStore.lua", 732, "items", "string", "map", "slot name -> item (ModStore.lua:694-711)"),
    ("Classes/ModStore.lua", 737, "items", "string", "map", "slot name -> item (ModStore.lua:694-711)"),
    ("Classes/ModStore.lua", 742, "items", "string", "map", "slot name -> item (ModStore.lua:694-711)"),
    ("Classes/ModStore.lua", 747, "items", "string", "map", "slot name -> item (ModStore.lua:694-711)"),
    ("Classes/ModStore.lua", 934, "self.actor.minionData.monsterTags", "int", "list", "minion monsterTags is a positional list"),
    # Classes/PartyTab.lua
    ("Classes/PartyTab.lua", 939, 'list["otherEffects"]', "string", "map", "buff names"),
    # Classes/PassiveSpec.lua
    ("Classes/PassiveSpec.lua", 108, "xml", "mixed", "", "an XML node: children at 1..n plus elem/attrib"),
    ("Classes/PassiveSpec.lua", 159, "xml", "mixed", "", "an XML node: children at 1..n plus elem/attrib"),
    ("Classes/PassiveSpec.lua", 170, "self.tree.tattoo.nodes", "string", "map", "Data/TattooPassives.lua nodes are keyed by name"),
    ("Classes/PassiveSpec.lua", 290, "hashList", "int", "list", "list of node ids"),
    ("Classes/PassiveSpec.lua", 1076, "item.jewelData.impossibleEscapeKeystones", "string", "map", "keystone names (Item.lua:2483)"),
    ("Classes/PassiveSpec.lua", 1118, "data.readAbyssJewelLUT(conqueredBy.id, socketId, jewelType, path)", "int", "map",
     "affectedNodes[nodeId] (DataAbyssJewelLookUpTableHelper.lua:234-267)"),
    ("Classes/PassiveSpec.lua", 1195, "impossibleEscapeKeystones", "string", "map", "keystone names (Item.lua:2483)"),
    ("Classes/PassiveSpec.lua", 1261, "changedNode.stats", "string", "map", "legion node stats keyed by stat id"),
    ("Classes/PassiveSpec.lua", 1327, "addition.stats", "string", "map", "legion addition stats keyed by stat id"),
    ("Classes/PassiveSpec.lua", 1812, "self.subGraphs", "int", "map", "self.subGraphs[nodeId] (PassiveSpec.lua:1997)"),
    ("Classes/PassiveSpec.lua", 1835, "self.jewel_data", "int", "map", "jewel_data[tonumber(key)] (ImportTab.lua:1141)"),
    ("Classes/PassiveSpec.lua", 1837, "value.subgraph.groups", "string", "map", "decoded JSON object keys"),
    ("Classes/PassiveSpec.lua", 1840, "value.subgraph.nodes", "string", "map", "decoded JSON object keys (tonumber(id) at 2059)"),
    ("Classes/PassiveSpec.lua", 1937, "indicies", "int", "map", "cluster orbit indices (nodeIndex arithmetic at 1954)"),
    ("Classes/PassiveSpec.lua", 1962, "legacyNodeIdsByOidx", "int", "map", "legacyNodeIdsByOidx[oidx] (PassiveSpec.lua:1956)"),
    ("Classes/PassiveSpec.lua", 2030, "importedGroups", "string", "map", "copied from value.subgraph.groups (JSON keys)"),
    ("Classes/PassiveSpec.lua", 2053, "importedNodes", "string", "map", "copied from value.subgraph.nodes (JSON keys)"),
    ("Classes/PassiveSpec.lua", 2460, "addition.mods", "int", "list", "parsed mod lines, positional"),
    ("Classes/PassiveSpec.lua", 2486, "keystoneNames", "string", "map", "impossibleEscapeKeystones (keystone names)"),
    # Classes/PassiveTree.lua
    ("Classes/PassiveTree.lua", 112, "assert(loadstring(treeText))()", "string", "map", "top-level tree.lua fields"),
    ("Classes/PassiveTree.lua", 151, "legacyAscMap", "string", "map", "legacyAscMap[ascendClass.id] (PassiveTree.lua:143)"),
    ("Classes/PassiveTree.lua", 164, "temp_groups", "int", "map", "temp_groups[node.group] (PassiveTree.lua:161)"),
    ("Classes/PassiveTree.lua", 167, "legacyAscMap", "string", "map", "legacyAscMap[ascendClass.id] (PassiveTree.lua:143)"),
    ("Classes/PassiveTree.lua", 290, "assert(loadstring(spriteText))()", "string", "map", "top-level sprites.lua fields"),
    ("Classes/PassiveTree.lua", 296, "self.skillSprites", "string", "map", "sprite type names"),
    ("Classes/PassiveTree.lua", 311, "maxZoom.coords", "string", "map", "sprite names"),
    ("Classes/PassiveTree.lua", 404, "data.coords", "string", "map", "sprite names"),
    ("Classes/PassiveTree.lua", 420, "maxZoom.coords", "string", "map", "sprite names"),
    ("Classes/PassiveTree.lua", 493, "self.nodeOverlay", "string", "map", "Normal/Notable/Keystone/... (PassiveTree.lua:~470-492)"),
    ("Classes/PassiveTree.lua", 501, "self.groups", "int", "map", "tree.lua groups are keyed [1].. by group id"),
    ("Classes/PassiveTree.lua", 559, "node.masteryEffects", "int", "list", "positional list in tree.lua"),
    ("Classes/PassiveTree.lua", 629, "node.out or {}", "int", "list", "positional list of id strings in tree.lua"),
    ("Classes/PassiveTree.lua", 647, 'node["in"] or {}', "int", "list", "positional list of id strings in tree.lua"),
    ("Classes/PassiveTree.lua", 726, "self.legion.additions", "int", "list", "LegionPassives.lua additions are [1]..[n]"),
    ("Classes/PassiveTree.lua", 759, "self.tattoo.nodes", "string", "map", "Data/TattooPassives.lua nodes are keyed by name"),
    # Classes/SkillsTab.lua, TreeTab.lua
    ("Classes/SkillsTab.lua", 474, "possibleVariants", "string", "map", "gemsByGameId[gemId] keyed by variantId (SkillsTab.lua:471)"),
    ("Classes/TreeTab.lua", 510, "xml", "mixed", "", "an XML node: children at 1..n plus elem/attrib"),
    # Data/Skills
    ("Data/Skills/act_dex.lua", 7173, "output.AreaOfEffectRadiusTertiaryOccurrences", "int", "map",
     "numeric radius keys (CalcOffence.lua:442)"),
    ("Data/Skills/act_dex.lua", 7187, "output.AreaOfEffectRadiusTertiaryOccurrences", "int", "map",
     "numeric radius keys (CalcOffence.lua:442); sorted after"),
    # Modules/Build.lua
    ("Modules/Build.lua", 2288, "self.savers", "string", "map", "XML section names; order decides the saved XML section order"),
    # Modules/CalcActiveSkill.lua
    ("Modules/CalcActiveSkill.lua", 58, "extraStats", "int", "list", "positional list (extraStats[1] test at 57)"),
    ("Modules/CalcActiveSkill.lua", 62, "stats", "string", "map", "stat ids (buildSkillInstanceStats)"),
    ("Modules/CalcActiveSkill.lua", 117, "supportEffect.grantedEffect.addSkillTypes", "int", "list", "list of SkillType numbers"),
    ("Modules/CalcActiveSkill.lua", 137, "supportEffect.grantedEffect.addSkillTypes", "int", "list", "list of SkillType numbers"),
    ("Modules/CalcActiveSkill.lua", 155, "supportEffect.grantedEffect.addFlags", "string", "map", "skill flag names"),
    ("Modules/CalcActiveSkill.lua", 258, "part", "string", "map", "a skill part: name plus flag fields"),
    ("Modules/CalcActiveSkill.lua", 351, "calcLib.buildSkillInstanceStats(activeEffect, activeGrantedEffect)", "string", "map", "stat ids"),
    # Modules/CalcDefence.lua
    ("Modules/CalcDefence.lua", 197, "damageTable", "string", "map", "damage type names"),
    ("Modules/CalcDefence.lua", 324, "damageTable", "string", "map", "damage type names"),
    ("Modules/CalcDefence.lua", 335, "damageTable", "string", "map", "damage type names"),
    ("Modules/CalcDefence.lua", 344, "damageTable", "string", "map", "damage type names"),
    ("Modules/CalcDefence.lua", 401, "damageTable", "string", "map", "damage type names"),
    # Modules/CalcOffence.lua
    ("Modules/CalcOffence.lua", 438, "baseRadiiOccurrences", "int", "map", "numeric radius keys (CalcOffence.lua:432)"),
    ("Modules/CalcOffence.lua", 455, "baseRadiiOccurrences", "int", "map", "numeric radius keys (CalcOffence.lua:432); sorted after"),
    ("Modules/CalcOffence.lua", 1958, "skillConv", "string", "map", "damage type names"),
    ("Modules/CalcOffence.lua", 1961, "globalConv", "string", "map", "damage type names"),
    ("Modules/CalcOffence.lua", 1967, "globalConv", "string", "map", "damage type names"),
    ("Modules/CalcOffence.lua", 1974, "globalConv", "string", "map", "damage type names"),
    # Modules/CalcPerform.lua
    ("Modules/CalcPerform.lua", 1269, "lifePools", "string", "map", "keyed by skill name / stat name (CalcPerform.lua:1256-1266)"),
    ("Modules/CalcPerform.lua", 1492, "minionCounts", "string", "map", "minionData.limit or '<id>:<type>' (CalcPerform.lua:1443)"),
    ("Modules/CalcPerform.lua", 1617, "env.flasks", "identity", "set", "env.flasks[item] = true"),
    ("Modules/CalcPerform.lua", 1825, "appliedFlaskBuffs", "string", "map", "mergeBuff(src, t, key) keys are names"),
    ("Modules/CalcPerform.lua", 1835, "flaskBuffsNonPlayer", "string", "map", "mergeBuff(src, t, key) keys are names"),
    ("Modules/CalcPerform.lua", 1843, 'flaskBuffsPerBaseNonPlayer["Quicksilver Flask"]', "string", "map",
     "mergeBuff(src, t, key) keys are names"),
    ("Modules/CalcPerform.lua", 1918, "tinctureBuffs", "string", "map", "mergeBuff(src, t, key) keys are names"),
    ("Modules/CalcPerform.lua", 1922, "tinctureBuffs", "string", "map", "mergeBuff(src, t, key) keys are names"),
    ("Modules/CalcPerform.lua", 2249, "modData.value", "int", "list", "LIST mod value: a list of mods"),
    ("Modules/CalcPerform.lua", 2257, "modData.value", "int", "list", "LIST mod value: a list of mods"),
    ("Modules/CalcPerform.lua", 2264, "modData.value", "int", "list", "LIST mod value: a list of mods"),
    ("Modules/CalcPerform.lua", 3198, 'allyPartyCurses["Curse"]', "string", "map", "curse names"),
    ("Modules/CalcPerform.lua", 3449, 'allyBuffs["extraAura"]', "string", "map", "PartyTab buff export entry (extraAura)"),
    # Modules/CalcSetup.lua
    ("Modules/CalcSetup.lua", 261, "env.radiusJewelList", "int", "list", "built with t_insert (CalcSetup.lua:887/914)"),
    ("Modules/CalcSetup.lua", 273, "nodeList", "int", "map", "env.allocNodes (CalcSetup.lua:774/1421): node-id map, order observable"),
    ("Modules/CalcSetup.lua", 292, "env.radiusJewelList", "int", "list", "built with t_insert (CalcSetup.lua:887/914)"),
    ("Modules/CalcSetup.lua", 908, "data.jewelRadius", "int", "list", "data.jewelRadius is a positional list"),
    ("Modules/CalcSetup.lua", 983, "itemDisablers", "string", "map", "slot names (CalcSetup.lua:955/973)"),
    ("Modules/CalcSetup.lua", 1004, "trueDisabled", "string", "map", "slot names"),
    ("Modules/CalcSetup.lua", 1807, "supportLists[slotName]", "identity", "map", "supportLists[slot][group] (CalcSetup.lua:1621)"),
    ("Modules/CalcSetup.lua", 1815, "env.crossLinkedSupportGroups", "string", "map", "keyed by sourceSlot (CalcSetup.lua:1596)"),
    ("Modules/CalcSetup.lua", 1827, "supportLists[crossLinkedSupportSlot]", "identity", "map",
     "supportLists[slot][group] (CalcSetup.lua:1621)"),
    # Modules/CalcTools.lua, Calcs.lua
    ("Modules/CalcTools.lua", 63, "checkTypes", "int", "list", "a type expression: list of SkillType numbers"),
    ("Modules/Calcs.lua", 28, "t", "generic", "", "math.average over any table"),
    ("Modules/Calcs.lua", 123, "gemList", "int", "list", "socket group gemList is a list"),
    ("Modules/Calcs.lua", 364, "env.limitedSkills or {}", "string", "set", "cacheSkillUUID strings (CalcMirages.lua:45)"),
    # Modules/Common.lua
    ("Modules/Common.lua", 479, "tbl", "generic", "", "copyTable"),
    ("Modules/Common.lua", 498, "tbl", "generic", "", "copyTableSafe"),
    ("Modules/Common.lua", 553, "tbl", "generic", "", "wipeTable"),
    # Modules/ConfigOptions.lua, Data.lua
    ("Modules/ConfigOptions.lua", 2335, "additionalStats", "string", "map", "boss additionalStats: mod names"),
    ("Modules/Data.lua", 1055, "list", "int", "list", "grantedEffect baseMods/qualityMods/levelMods are positional"),
    # Modules/DataAbyss..., DataLegion...
    ("Modules/DataAbyssJewelLookUpTableHelper.lua", 61, "node.stats", "string", "map", "stat ids"),
    ("Modules/DataLegionLookUpTableHelper.lua", 59, "data.nodeIDList", "mixed", "",
     "node ids plus size/localIdToGlobalId (DataLegionLookUpTableHelper.lua:50/91/166)"),
    # Modules/ItemTools.lua, ModParser.lua, ModTools.lua, PantheonTools.lua, Utils.lua
    ("Modules/ItemTools.lua", 325, "modList", "int", "list", "parseMod result list"),
    ("Modules/ModParser.lua", 22, "srcList", "int", "list", "getSimpleConv({ ... }) positional lists"),
    ("Modules/ModParser.lua", 6037, "inputTable", "string", "map", "keyed by sub-line text"),
    ("Modules/ModParser.lua", 6597, "data.clusterJewels.jewels", "string", "map", "keyed by base name"),
    ("Modules/ModParser.lua", 6621, "patternList", "string", "map", "Lua patterns; full ties are random in PoB (calc doc §3.4)"),
    ("Modules/ModParser.lua", 6912, "data", "mixed", "", "modExtraTags data: tag/tagList fields and positional entries"),
    ("Modules/ModTools.lua", 138, "src", "string", "map", "ModFlag/KeywordFlag names"),
    ("Modules/PantheonTools.lua", 8, "soul.mods", "int", "list", "Data/Pantheons.lua mods are [1]..[n]"),
    ("Modules/PantheonTools.lua", 11, "modList", "int", "list", "parseMod result list"),
    ("Modules/Utils.lua", 43, "value", "generic", "", "writeStringify over any table"),
]

if __name__ == "__main__":
    sys.exit(main())
