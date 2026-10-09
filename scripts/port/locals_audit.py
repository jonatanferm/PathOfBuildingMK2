#!/usr/bin/env python3
"""Measure the cross-segment locals of the split calc functions (calc doc §7.1, D6).

A scope-aware analyser: every Lua file is tokenized and parsed (luaast.py), every
identifier is resolved to its declaration with Lua's scoping rules, and a local counts
as *crossing* when it is declared in one segment and referenced (read or written,
including from a nested closure) in another. Segments come from segments.py.

Locals whose scope is the function body (and the function's parameters) form the
function's ctx; locals scoped to a split loop (its control variables and body locals)
form that loop's locals struct. A crossing local in any other scope would mean a
boundary cuts through a nested block, which segments.py forbids; it is reported.

Parameters follow the *params rule*: the orchestrator receives them, so a parameter is
a ctx field when it crosses, and also when it does not cross but no function-body local
of the same name shadows it. (initEnv's `override` is shadowed by
`local override = override or { }`, CalcSetup.lua:485, and only that local is a field.)
At the pin one listed parameter is not itself crossing: `env` of calcs.defence
(referenced only in 651-1210); --check prints it as a note.

Usage:
  locals_audit.py                 print the §7.1 table (markdown rows)
  locals_audit.py --check [DOC]   compare with the table in docs/port/calc-architecture.md
                                  (exit 1 on any difference)
  locals_audit.py --json          machine-readable result
  locals_audit.py --file F --header N --starts a,b,c [--loop H:E:s1,s2]
                                  ad-hoc audit of any function
"""

import argparse
import json
import os
import re
import sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))

import pobpaths  # noqa: E402
import segments  # noqa: E402


class Audit:
    def __init__(self, sf):
        self.sf = sf
        self.params = []   # ctx params (Decl), by the params rule
        self.top = []      # crossing function-body locals (Decl), by line
        self.loops = {}    # loop header line -> [Decl]
        self.other = []    # crossing locals in any other scope (anomalies)
        self.crossing_segments = {}  # Decl.uid -> sorted segment starts referencing it
        self.uncrossed_params = []   # params listed by the params rule but referenced in one segment only


def _within(decl, func):
    """True if decl is declared lexically inside func (or is one of its params)."""
    f = decl.func
    while f is not None:
        if f is func:
            return True
        f = getattr(f, "parent_func", None)
    return False


def audit_function(sf, chunk, res):
    func = segments.find_function(res, sf.header)
    loops = {}
    for lp in sf.loops:
        loops[lp.header] = segments.find_loop(func, lp.header)
    starts = sf.all_starts()
    a = Audit(sf)
    for lp in sf.loops:
        a.loops[lp.header] = []
    for d in res.decls:
        if not _within(d, func):
            continue
        ds = sf.segment_of(d.line)
        segs = {sf.segment_of(r.line) for r in d.refs}
        if d.scope is func:
            # Parameters (params rule, see module doc): a field when crossing, or when not
            # shadowed by a function-body local of the same name. Whether it was measured
            # as crossing is recorded in uncrossed_params / --json.
            shadowed = any(o.scope is func.body and o.name == d.name for o in res.decls if _within(o, func))
            a.crossing_segments[d.uid] = sorted(starts[s] for s in segs | {ds})
            crossing = bool(segs - {ds})
            if crossing or not shadowed:
                a.params.append(d)
                if not crossing:
                    a.uncrossed_params.append(d)
            continue
        if not (segs - {ds}):
            continue
        a.crossing_segments[d.uid] = sorted(starts[s] for s in segs | {ds})
        if d.scope is func.body:
            a.top.append(d)
        else:
            for h, loop in loops.items():
                if d.scope is loop or d.scope is loop.body:
                    a.loops[h].append(d)
                    break
            else:
                a.other.append(d)
    key = lambda d: (d.line, d.uid)  # noqa: E731
    a.params.sort(key=lambda d: [p[0] for p in func.params].index(d.name))
    a.top.sort(key=key)
    for h in a.loops:
        a.loops[h].sort(key=key)
    a.other.sort(key=key)
    return a


def render_decls(decls, params=()):
    """Render a crossing list the way calc doc §7.1 writes it.

    Parameters come first as "params `a b`". A run of three or more consecutive local
    functions is written as a "local fns ..." group between semicolons; any other local
    function is written inline with a "(fn)" suffix.
    """
    groups = []
    if params:
        groups.append("params `%s`" % " ".join(d.name for d in params))
    i = 0
    cur = []
    while i < len(decls):
        j = i
        while j < len(decls) and decls[j].kind == "localfn":
            j += 1
        if j - i >= 3:
            if cur:
                groups.append(" ".join(cur))
                cur = []
            groups.append("local fns " + " ".join("`%s`%d" % (d.name, d.line) for d in decls[i:j]))
            i = j
            continue
        d = decls[i]
        cur.append("`%s`%d%s" % (d.name, d.line, " (fn)" if d.kind == "localfn" else ""))
        i += 1
    if cur:
        groups.append(" ".join(cur))
    return "; ".join(groups)


def table_rows(audits):
    rows = []
    for a in audits:
        sf = a.sf
        lo, hi = segments.DOC_RANGE.get(sf.id, (sf.header, sf.end))
        rows.append("| `%s` %d-%d | %s | %s |" % (sf.name, lo, hi, sf.starts_text(), render_decls(a.top, a.params)))
        for lp in sf.loops:
            rows.append("| %s body %d-%d, split at %s | | %s |" % (
                lp.label, lp.header, lp.end, ", ".join(str(s) for s in lp.splits), render_decls(a.loops[lp.header])))
    return rows


def doc_rows(doc_path):
    """The data rows of the §7.1 table in the calc architecture doc."""
    with open(doc_path, encoding="utf-8") as fh:
        text = fh.read()
    m = re.search(r"^### 7\.1 .*?$(.*?)^### 7\.2 ", text, re.S | re.M)
    if not m:
        raise SystemExit("§7.1 not found in %s" % doc_path)
    rows = []
    for ln in m.group(1).splitlines():
        ln = ln.strip()
        if ln.startswith("|") and not ln.startswith("|---") and not ln.startswith("| Function"):
            rows.append(ln)
    return rows


_ITEM = re.compile(r"`([A-Za-z_][A-Za-z_0-9]*)`(\d+)")
_PARAMS = re.compile(r"params `([^`]*)`")


def row_items(row):
    cells = [c.strip() for c in row.strip("|").split("|")]
    last = cells[-1]
    params = set()
    pm = _PARAMS.search(last)
    if pm:
        params = set(pm.group(1).split())
    return cells, params, set((n, int(ln)) for n, ln in _ITEM.findall(last))


def run_all(pob_src):
    audits = []
    for sf in segments.FUNCTIONS:
        chunk, res = segments.load(pob_src, sf.file)
        audits.append(audit_function(sf, chunk, res))
    return audits


def check(audits, doc_path):
    ours = table_rows(audits)
    theirs = doc_rows(doc_path)
    ok = True
    if len(ours) != len(theirs):
        print("FAIL row count: measured %d, doc %d" % (len(ours), len(theirs)))
        ok = False
    for o, t in zip(ours, theirs):
        if o == t:
            continue
        ok = False
        oc, op, oi = row_items(o)
        tc, tp, ti = row_items(t)
        print("FAIL row differs:\n  measured: %s\n  doc:      %s" % (o, t))
        if oc[:2] != tc[:2]:
            print("  label/starts differ: %r vs %r" % (oc[:2], tc[:2]))
        if op != tp:
            print("  params: measured-only %s, doc-only %s" % (sorted(op - tp), sorted(tp - op)))
        if oi != ti:
            print("  locals: measured-only %s, doc-only %s" % (sorted(oi - ti), sorted(ti - oi)))
    for a in audits:
        for d in a.other:
            ok = False
            print("FAIL %s: local %s@%d crosses a boundary from a nested scope" % (a.sf.name, d.name, d.line))
    for a in audits:
        for d in a.uncrossed_params:
            print("note %s: param `%s` is a ctx field by the params rule but is referenced only in "
                  "segment %d" % (a.sf.name, d.name, a.crossing_segments[d.uid][0]))
    if ok:
        n = sum(len(a.top) + len(a.params) + sum(len(v) for v in a.loops.values()) for a in audits)
        print("ok: %d rows, %d crossing locals identical to %s" % (len(ours), n, os.path.relpath(doc_path)))
    return 0 if ok else 1


def to_json(audits):
    out = []
    for a in audits:
        def dj(d):
            return {"name": d.name, "line": d.line, "kind": d.kind, "segments": a.crossing_segments[d.uid],
                    "measured_crossing": d not in a.uncrossed_params}
        out.append({
            "function": a.sf.name, "file": a.sf.file, "header": a.sf.header, "end": a.sf.end,
            "starts": a.sf.all_starts(),
            "params": [dj(d) for d in a.params],
            "locals": [dj(d) for d in a.top],
            "loops": [{"header": lp.header, "end": lp.end, "splits": lp.splits, "label": lp.label,
                       "locals": [dj(d) for d in a.loops[lp.header]]} for lp in a.sf.loops],
            "anomalies": [dj(d) for d in a.other],
        })
    return out


def main(argv=None):
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--pob", help="PathOfBuilding checkout (contains src/)")
    ap.add_argument("--check", nargs="?", const="", metavar="DOC",
                    help="compare with the §7.1 table of DOC (default docs/port/calc-architecture.md)")
    ap.add_argument("--json", action="store_true")
    ap.add_argument("--file", help="ad-hoc: Lua file relative to src/ (or absolute)")
    ap.add_argument("--header", type=int, help="ad-hoc: line of the function header")
    ap.add_argument("--starts", help="ad-hoc: comma-separated top-level segment starts")
    ap.add_argument("--loop", action="append", default=[], help="ad-hoc: HEADER:END:split1,split2")
    args = ap.parse_args(argv)

    if args.file:
        pob_src = os.path.join(pobpaths.find_pob(args.pob), "src") if not os.path.isabs(args.file) else "/"
        rel = args.file
        chunk, res = segments.load(pob_src, rel)
        func = segments.find_function(res, args.header)
        starts = [int(x) for x in args.starts.split(",")] if args.starts else [args.header]
        if starts[0] != args.header:
            starts.insert(0, args.header)
        loops = []
        for spec in args.loop:
            h, e, sp = spec.split(":")
            loops.append(segments.Loop("loop", int(h), int(e), [int(x) for x in sp.split(",") if x]))
            if int(h) not in starts:
                starts.append(int(h))
        starts.sort()
        name = getattr(func, "decl_name", None) or "<function>"
        sf = segments.SplitFunction("adhoc", rel, name, args.header, func.endline, starts, loops)
        audits = [audit_function(sf, chunk, res)]
    else:
        pob_src = os.path.join(pobpaths.find_pob(args.pob), "src")
        audits = run_all(pob_src)

    if args.check is not None:
        doc = args.check or os.path.join(pobpaths.docs_port(), "calc-architecture.md")
        return check(audits, doc)
    if args.json:
        json.dump(to_json(audits), sys.stdout, indent=1)
        print()
        return 0
    print("| Function | Segment starts (inclusive) | Crossing locals (declaration line) |")
    print("|---|---|---|")
    for r in table_rows(audits):
        print(r)
    for a in audits:
        for d in a.other:
            print("WARNING: %s: %s@%d crosses a boundary from a nested scope" % (a.sf.name, d.name, d.line), file=sys.stderr)
    return 0


if __name__ == "__main__":
    sys.exit(main())
