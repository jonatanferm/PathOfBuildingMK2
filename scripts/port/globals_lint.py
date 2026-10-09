#!/usr/bin/env python3
"""List reads of undeclared globals in the calc scope (calc doc D16, §7.3).

A name is *declared* as a global when any PoB runtime file assigns it at any scope
(`x = ...`, `function x()`, `_G.x = ...`, `_G["x"] = ...`, `rawset(_G, "x", ...)`), when
SimpleGraphic provides it (`src/_SimpleGraphic.def.lua`, `HeadlessWrapper.lua`), or when
it is a Lua 5.1 / LuaJIT 2.1 standard global. A read of any other global in the calc
scope always yields nil at runtime: those are the D16 sites, ported as literal nil with
`// LUA-GLOBAL-NIL`.

Runtime files: src/**/*.lua and runtime/lua/**/*.lua, except src/Export/** (the GGPK
exporter, not loaded by PoB), src/TreeData/** (pure data tables) and Lua 5.3-only
helpers that LuaJIT cannot load (runtime/lua/sha1/lua53_ops.lua).

The second report section lists *implicit globals*: assignments to an undeclared-local
name inside a function body in the calc scope (they create or overwrite a global, which
persists across calls); the third section lists the reads of those names. Global
functions defined at the top level of a chunk on purpose (calc doc §13) are not listed.

Usage:
  globals_lint.py [--scope FILE] [--json]
  globals_lint.py --check [--doc FILE]
                              exit 1 unless the undeclared-read sites equal the ones calc doc
                              §7.3 lists (parsed from the doc) plus CCR-0004
"""

import argparse
import json
import re
import os
import sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))

import calcscope  # noqa: E402
import luaast  # noqa: E402
import pobpaths  # noqa: E402

LUA_GLOBALS = frozenset("""
_G _VERSION assert collectgarbage dofile error gcinfo getfenv getmetatable ipairs load loadfile
loadstring module newproxy next pairs pcall print rawequal rawget rawlen rawset require select
setfenv setmetatable tonumber tostring type unpack xpcall
coroutine debug io math os package string table bit jit arg
""".split())

# calc doc §7.3 as of this writing: the undeclared-global read sites at the pin (D16). `--check`
# reads the sites from the doc itself (doc_sites()); this copy only guards the doc parser in tests.
KNOWN_DOC_SITES = {
    ("Modules/CalcOffence.lua", 3527, "dotCfg"),
    ("Modules/CalcOffence.lua", 3538, "dotCfg"),
    ("Modules/CalcPerform.lua", 3464, "affectedByAura"),
    ("Modules/CalcDefence.lua", 1606, "modSource"),
    ("Classes/ModDB.lua", 262, "nullValue"),
    ("Classes/ModList.lua", 207, "nullValue"),
}

_SITE_GROUP = re.compile(r"`([A-Za-z_][A-Za-z_0-9]*)` \(([^)]*)\)")
_SITE_REF = re.compile(r"(?:\b([A-Z][A-Za-z0-9_]*)(?:\.lua)?(?::|\s+))?(\d+)")


def doc_sites(doc_path, pob):
    """The undeclared-global read sites that calc doc §7.3 lists, as (path, line, name).

    §7.3 writes them in prose: `name` (Module line[, line]; ...) or `name` (File.lua:line, ...).
    Only the bullet(s) that mention undeclared globals are read. A module name is resolved to
    Modules/<X>.lua, else Classes/<X>.lua, else Data/<X>.lua under the checkout's src/."""
    with open(doc_path, encoding="utf-8") as fh:
        text = fh.read()
    m = re.search(r"^### 7\.3 .*?$(.*?)^##? ", text, re.S | re.M)
    if not m:
        raise SystemExit("§7.3 not found in %s" % doc_path)
    sites = set()
    for para in m.group(1).split("\n- "):
        if "undeclared" not in para:
            continue
        for name, inner in _SITE_GROUP.findall(para):
            module = None
            for mod, line in _SITE_REF.findall(inner.split(";")[0]):
                module = mod or module
                if module is None:
                    continue
                for d in ("Modules", "Classes", "Data"):
                    rel = "%s/%s.lua" % (d, module)
                    if os.path.isfile(os.path.join(pob, "src", rel)):
                        sites.add((rel, int(line), name))
                        break
                else:
                    raise SystemExit("§7.3: cannot resolve %r (site of `%s`)" % (module, name))
    if not sites:
        raise SystemExit("§7.3 of %s lists no undeclared-global sites" % doc_path)
    return sites

# CCR-0004 (docs/port/ccr/0004-p4-undeclared-global-sites.md): further undeclared-global reads
# in the calc scope at the pin that §7.3 does not list. Each was verified by hand.
CCR_0004_SITES = {
    ("Classes/PartyTab.lua", 919, "aura"),                       # loop var of 911-917 is out of scope
    ("Data/Skills/act_dex.lua", 12991, "cfg"),                   # Scourge Arrow preDamageFunc
    ("Modules/CalcBreakdown.lua", 212, "skillCfg"),              # breakdown.leech
    ("Modules/CalcBreakdown.lua", 232, "skillCfg"),
    ("Modules/CalcOffence.lua", 2506, "cfg"),                    # Mantra of Flames BuffOnSelf
    ("Modules/CalcOffence.lua", 2507, "cfg"),
    ("Modules/CalcPerform.lua", 915, "skillCfg"),                # doActorMisc rage; local at 661 is out of scope
    ("Modules/DataLegionLookUpTableHelper.lua", 136, "scriptPath"),  # repairLUTs; local only in UpdateCheck.lua
    ("Modules/DataLegionLookUpTableHelper.lua", 144, "scriptPath"),
}

EXCLUDE_DIRS = ("Export", "TreeData")
EXCLUDE_FILES = ("runtime/lua/sha1/lua53_ops.lua",)


def runtime_files(pob):
    out = []
    for base in ("src", os.path.join("runtime", "lua")):
        root = os.path.join(pob, base)
        for dirpath, dirnames, filenames in os.walk(root):
            rel_dir = os.path.relpath(dirpath, os.path.join(pob, "src"))
            if base == "src" and rel_dir.split(os.sep)[0] in EXCLUDE_DIRS:
                dirnames[:] = []
                continue
            dirnames.sort()
            for fn in sorted(filenames):
                if not fn.endswith(".lua"):
                    continue
                full = os.path.join(dirpath, fn)
                if os.path.relpath(full, pob).replace(os.sep, "/") in EXCLUDE_FILES:
                    continue
                out.append(full)
    return out


def src_rel(pob, full):
    return os.path.relpath(full, os.path.join(pob, "src")).replace(os.sep, "/")


def _is_G(e):
    return e.kind == "Name" and e.name == "_G" and getattr(e, "decl", None) is None


def global_definitions(chunk, res):
    """Names this file assigns as globals."""
    names = set()
    for name, ref in res.globals:
        if ref.write:
            names.add(name)
    for n in luaast.walk(chunk):
        if n.kind == "Assign":
            for t in n.targets:
                if t.kind == "Index" and _is_G(t.obj) and t.key.kind == "String":
                    names.add(t.key.value)
        elif n.kind == "Call" and n.func.kind == "Name" and n.func.name == "rawset" and len(n.args) >= 2:
            if _is_G(n.args[0]) and n.args[1].kind == "String":
                names.add(n.args[1].value)
    return names


def lint(pob, scope, cache=None):
    files = runtime_files(pob)
    parsed = {}
    declared = set(LUA_GLOBALS)
    for full in files:
        rel = src_rel(pob, full)
        try:
            chunk = luaast.parse_file(full)
            res = luaast.resolve(chunk)
        except luaast.LuaSyntaxError as e:
            print("warning: skipped %s: %s" % (rel, e), file=sys.stderr)
            continue
        declared |= global_definitions(chunk, res)
        if scope.files() and rel in scope.files():
            parsed[rel] = (chunk, res)
    reads = []
    implicit = []
    for rel in scope.files():
        if rel not in parsed:
            full = os.path.normpath(os.path.join(pob, "src", rel))
            chunk = luaast.parse_file(full)
            parsed[rel] = (chunk, luaast.resolve(chunk))
        chunk, res = parsed[rel]
        for name, ref in res.globals:
            if not scope.contains(rel, ref.line):
                continue
            if not ref.write and name not in declared:
                reads.append({"file": rel, "line": ref.line, "name": name,
                              "function": luaast.enclosing_function_name(ref.func)})
            elif ref.write and ref.func is not None:
                implicit.append({"file": rel, "line": ref.line, "name": name,
                                 "function": luaast.enclosing_function_name(ref.func)})
    implicit_names = {r["name"] for r in implicit}
    implicit_reads = []
    for rel in scope.files():
        chunk, res = parsed[rel]
        for name, ref in res.globals:
            if name in implicit_names and not ref.write and scope.contains(rel, ref.line):
                implicit_reads.append({"file": rel, "line": ref.line, "name": name,
                                       "function": luaast.enclosing_function_name(ref.func)})
    reads.sort(key=lambda r: (r["file"], r["line"], r["name"]))
    implicit.sort(key=lambda r: (r["file"], r["line"], r["name"]))
    implicit_reads.sort(key=lambda r: (r["file"], r["line"], r["name"]))
    # Collapse repeated reads of one name on one line.
    seen = set()
    uniq = []
    for r in reads:
        k = (r["file"], r["line"], r["name"])
        if k not in seen:
            seen.add(k)
            uniq.append(r)
    return uniq, implicit, implicit_reads


def main(argv=None):
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--pob", help="PathOfBuilding checkout")
    ap.add_argument("--scope", help="scope.toml (default docs/port/scope.toml, else the built-in default)")
    ap.add_argument("--json", action="store_true")
    ap.add_argument("--check", action="store_true", help="compare the undeclared reads with calc doc §7.3")
    ap.add_argument("--doc", help="calc architecture doc whose §7.3 --check reads (default docs/port/calc-architecture.md)")
    args = ap.parse_args(argv)
    pob = pobpaths.find_pob(args.pob)
    scope_path = args.scope or os.path.join(pobpaths.docs_port(), "scope.toml")
    scope = calcscope.load_scope(scope_path, calcscope.default_closures_json(pobpaths.main_checkout()),
                                 required=bool(args.scope))
    reads, implicit, implicit_reads = lint(pob, scope)
    if args.json:
        json.dump({"scope": scope.source, "undeclared_reads": reads, "implicit_globals": implicit,
                   "implicit_global_reads": implicit_reads}, sys.stdout, indent=1)
        print()
    elif not args.check:
        print("# Undeclared-global reads in the calc scope (always nil; port as `// LUA-GLOBAL-NIL`)")
        print("# scope: %s" % scope.source)
        for r in reads:
            print("%s:%d\t%s\t(in %s)" % (r["file"], r["line"], r["name"], r["function"]))
        print()
        print("# Implicit globals: assignments to a global inside a function body in the calc scope")
        for r in implicit:
            print("%s:%d\t%s\t(in %s)" % (r["file"], r["line"], r["name"], r["function"]))
        print()
        print("# Reads of those implicit globals in the calc scope (state that can leak across calls)")
        for r in implicit_reads:
            print("%s:%d\t%s\t(in %s)" % (r["file"], r["line"], r["name"], r["function"]))
    if args.check:
        got = {(r["file"], r["line"], r["name"]) for r in reads}
        expected = doc_sites(args.doc or os.path.join(pobpaths.docs_port(), "calc-architecture.md"), pob)
        allowed = expected | CCR_0004_SITES
        missing = allowed - got
        extra = got - allowed
        for m in sorted(missing):
            print("FAIL expected site not reported: %s:%d %s" % m)
        for x in sorted(extra):
            print("FAIL site in neither §7.3 nor CCR-0004: %s:%d %s" % x)
        if missing or extra:
            return 1
        fmt = lambda s: ", ".join("%s %s:%d" % (n, f.rsplit("/", 1)[-1], l) for f, l, n in sorted(s))  # noqa: E731
        print("ok: all %d calc doc §7.3 sites reported (%s)" % (len(expected), fmt(expected)))
        print("ok: %d further sites, all filed in CCR-0004 (%s)" % (len(CCR_0004_SITES), fmt(CCR_0004_SITES)))
    return 0


if __name__ == "__main__":
    sys.exit(main())
