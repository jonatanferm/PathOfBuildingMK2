#!/usr/bin/env python3
"""The calc crates' dependency graph, measured (docs/port/calc-split-plan.md §1.1, §4; CCR-0131).

  calc_depgraph.py graph OUT.json [--src DIR]
        Resolve every identifier and method call of the calc to the file that defines it
        (calc_depgraph_resolve.py). Default input: a merged view of the six calc crates
        (crates/pob-calc-*/src without their mirror-root lib.rs, plus a synthetic crate root), so
        the graph is the same file graph the plan measured on the monolith. --src DIR resolves
        one src/ tree instead (e.g. a pre-split checkout's crates/pob-calc/src).
  calc_depgraph.py check [GRAPH.json]
        Every non-test file edge must point into the file's own crate or a crate it depends on
        (the pob-calc-* [dependencies] of crates/pob-calc-*/Cargo.toml, transitively). Prints the
        wrong-way edges with their symbols; exit 1 if there are any. Without GRAPH.json it runs
        `graph` into a temporary file first.
  calc_depgraph.py external-paths
        The `pob_calc::` paths outside the calc crates (crates/, app/; code only, `use` brace
        trees expanded to one path per leaf, aliases dropped), grouped by the crate that serves
        them through the pob-calc facade.
"""
import collections
import json
import os
import re
import runpy
import sys
import tempfile

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))

import pobpaths  # noqa: E402

ROOT = pobpaths.repo_root()
CRATES = ["core", "setup", "config", "defence", "offence", "perform"]


def crate_dir(c):
    return os.path.join(ROOT, "crates", "pob-calc-" + c)


def crate_deps():
    """crate -> set of calc crates it depends on (transitively), from the manifests."""
    direct = {}
    for c in CRATES:
        text = open(os.path.join(crate_dir(c), "Cargo.toml")).read()
        sec = re.search(r"^\[dependencies\]\n(.*?)(?=^\[)", text, re.S | re.M)
        direct[c] = set(re.findall(r"^pob-calc-([a-z]+)", sec.group(1) if sec else "", re.M))

    def closure(c, seen):
        for d in direct[c]:
            if d not in seen:
                seen.add(d)
                closure(d, seen)
        return seen

    return {c: closure(c, set()) for c in CRATES}


def merged_view(tmp):
    """Symlink every calc source file at its src-relative path; write a synthetic lib.rs and
    modules/mod.rs that declare every module. → {rel path: crate}."""
    owner = {}
    for c in CRATES:
        src = os.path.join(crate_dir(c), "src")
        for d, _, fs in os.walk(src):
            for f in fs:
                if not f.endswith(".rs"):
                    continue
                rel = os.path.relpath(os.path.join(d, f), src)
                if rel in ("lib.rs", os.path.join("modules", "mod.rs")):
                    continue
                assert rel not in owner, (rel, owner.get(rel), c)
                owner[rel] = c
                dst = os.path.join(tmp, rel)
                os.makedirs(os.path.dirname(dst), exist_ok=True)
                os.symlink(os.path.join(d, f), dst)

    def children(sub):
        names = set()
        for rel in owner:
            parts = rel.split(os.sep)
            if parts[:len(sub)] == sub and len(parts) > len(sub):
                n = parts[len(sub)]
                names.add(n[:-3] if n.endswith(".rs") else n)
        return sorted(names - {"mod"})

    with open(os.path.join(tmp, "lib.rs"), "w") as fh:
        fh.write("".join("pub mod %s;\n" % m for m in children([]) + ["modules"]))
        fh.write("pub use calcs_facade::calcs;\npub use env::{EnvRef, Mode};\npub use flow::Flow;\n"
                 "pub use lua_table::{Key, LStr, Output, Table, Val};\npub use model::{Build, BuildRef};\n")
    with open(os.path.join(tmp, "modules", "mod.rs"), "w") as fh:
        fh.write("".join("pub mod %s;\n" % m for m in children(["modules"])))
    owner["lib.rs"] = "facade"
    owner[os.path.join("modules", "mod.rs")] = "core"
    return owner


def run_resolver(src, out):
    argv = sys.argv
    sys.argv = ["calc_depgraph_resolve.py", src, out]
    try:
        runpy.run_path(os.path.join(os.path.dirname(os.path.abspath(__file__)), "calc_depgraph_resolve.py"),
                       run_name="__main__")
    finally:
        sys.argv = argv


def cmd_graph(out, src=None):
    if src:
        run_resolver(src, out)
        return None
    with tempfile.TemporaryDirectory() as tmp:
        owner = merged_view(tmp)
        run_resolver(tmp, out)
    g = json.load(open(out))
    g["crate_of"] = owner
    json.dump(g, open(out, "w"), indent=1)
    return g


def cmd_check(path):
    if path:
        g = json.load(open(path))
    else:
        with tempfile.TemporaryDirectory() as t:
            g = cmd_graph(os.path.join(t, "graph.json"))
    owner = g["crate_of"]
    deps = crate_deps()
    deps["facade"] = set(CRATES)
    bad = []
    n = 0
    for kind in ("refs", "method_refs"):
        for e in g[kind]:
            n += 1
            a, b = owner.get(e["src"]), owner.get(e["dst"])
            if a is None or b is None or a == b or b in deps.get(a, set()):
                continue
            bad.append((kind, e["src"], a, e["dst"], b, e["syms"][:6]))
    for kind, s, a, d, b, syms in bad:
        print("WRONG-WAY %s: %s [%s] -> %s [%s]: %s" % (kind, s, a, d, b, ", ".join("%s %d" % (x, c) for x, c in syms)))
    print("%s: %d file edges, %d point the wrong way" % ("ok" if not bad else "FAIL", n, len(bad)))
    return 1 if bad else 0


def expand(s, i, prefix, out):
    """Parse a use tree at s[i:] after `prefix::`; append one path per leaf; → index after it."""
    word = re.compile(r"\s*([A-Za-z_][A-Za-z0-9_]*|\{|\*)")
    while True:
        m = word.match(s, i)
        if not m:
            out.append(prefix)
            return i
        tok, i = m.group(1), m.end()
        if tok == "{":
            while True:
                i = expand(s, i, prefix, out)
                m2 = re.compile(r"\s*(,|\})").match(s, i)
                if not m2:
                    return i
                i = m2.end()
                if m2.group(1) == "}":
                    return i
                m3 = re.compile(r"\s*\}").match(s, i)
                if m3:
                    return m3.end()
        if tok == "*":
            out.append(prefix + "::*")
            return i
        name = prefix if tok == "self" else prefix + "::" + tok
        m3 = re.compile(r"\s*::").match(s, i)
        if m3:
            prefix, i = name, m3.end()
            continue
        m4 = re.compile(r"\s+as\s+[A-Za-z_][A-Za-z0-9_]*").match(s, i)
        if m4:
            i = m4.end()
        out.append(name)
        return i


SERVED = {  # pob_calc::modules::<m> -> crate (the facade's `modules` re-exports)
    "calcs": "perform", "power": "perform", "calc_mirages": "perform", "calc_perform": "perform",
    "calc_triggers": "perform", "calc_offence": "offence", "calc_setup": "setup",
    "calc_active_skill": "setup", "config_options": "config", "mod_map": "config",
    "calc_defence": "defence",
}


def cmd_external_paths():
    skip = [os.path.join(ROOT, "crates", d, "src") for d in ["pob-calc"] + ["pob-calc-" + c for c in CRATES]]
    files = []
    for top in ("crates", "app"):
        for d, dn, fs in os.walk(os.path.join(ROOT, top)):
            dn[:] = [x for x in dn if x != "target"]
            if any(d == s or d.startswith(s + os.sep) for s in skip):
                continue
            files += [os.path.join(d, f) for f in fs if f.endswith(".rs")]
    paths = []
    for f in files:
        text = open(f, encoding="utf-8", errors="replace").read()
        code = "\n".join(l for l in text.split("\n") if not l.lstrip().startswith("//"))
        for m in re.finditer(r"\bpob_calc::", code):
            expand(code, m.end(), "pob_calc", paths)
    by = collections.Counter()
    for p in paths:
        seg = p.split("::")
        s1 = seg[1] if len(seg) > 1 else ""
        s2 = seg[2] if len(seg) > 2 else ""
        if s1 == "modules":
            c = SERVED.get(s2, "core")
        elif s1 == "data_fns":
            c = "offence"
        elif s1 == "probe" and s2 == "run_segment":
            c = "perform (probe::run_segment)"
        elif s1 in ("calcs", "calcs_facade", "try_buildOutput"):
            c = "perform (calcs_facade)"
        elif s1 in ("Key", "LStr", "Table", "Val", "Output", "Build", "BuildRef", "EnvRef", "Mode", "Flow"):
            c = "root re-exports"
        else:
            c = "core"
        by[c] += 1
    print("files scanned: %d; %d pob_calc:: references, %d distinct paths" % (len(files), len(paths), len(set(paths))))
    for c, n in by.most_common():
        print("  %-30s %d" % (c, n))
    return 0


def main():
    a = sys.argv[1:]
    if not a or a[0] in ("-h", "--help"):
        print(__doc__)
        return 0
    if a[0] == "graph" and len(a) >= 2:
        src = a[a.index("--src") + 1] if "--src" in a else None
        cmd_graph(a[1], src)
        return 0
    if a[0] == "check":
        return cmd_check(a[1] if len(a) > 1 else None)
    if a[0] == "external-paths":
        return cmd_external_paths()
    print(__doc__)
    return 2


if __name__ == "__main__":
    sys.exit(main())
