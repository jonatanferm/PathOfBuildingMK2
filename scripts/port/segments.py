#!/usr/bin/env python3
"""Segment table of the giant calc functions (calc-architecture.md §7, D6).

This module is the single source of truth for where `calcs.offence`, `calcs.perform`,
`calcs.defence`, `calcs.buildDefenceEstimations` and `calcs.initEnv` are split, and
which unit owns each segment. `locals_audit.py` measures the crossing locals from it.

Usage:
  segments.py table                 print every segment with its owner and line range
  segments.py check                 verify every boundary is a statement boundary at the pin
  segments.py stages [--out FILE]   emit a draft `stages.toml` (U0 owns the real file):
                                    one probe per boundary, with the ctx locals live there
Options: --pob DIR  (PathOfBuilding checkout; default: see pobpaths.find_pob)
"""

import argparse
import os
import sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))

import luaast  # noqa: E402
import pobpaths  # noqa: E402


class Loop:
    """A loop whose body is split into sub-segments (the pass/ailment loops of offence)."""

    def __init__(self, label, header, end, splits):
        self.label = label        # e.g. "pass loop"
        self.header = header      # line of the `for` statement (first sub-segment start)
        self.end = end            # line of the loop's `end`
        self.splits = splits      # further sub-segment starts inside the body

    @property
    def starts(self):
        return [self.header] + list(self.splits)


class SplitFunction:
    def __init__(self, fid, file, name, header, end, starts, loops=(), units=()):
        self.id = fid
        self.file = file          # path relative to PathOfBuilding/src
        self.name = name          # Lua name, e.g. "calcs.offence"
        self.header = header      # line of `function calcs.x(...)`
        self.end = end            # line of the function's final `end`
        self.starts = starts      # top-level segment starts (inclusive), loops given separately
        self.loops = list(loops)
        self.units = list(units)  # (unit, first, last, note) ownership per §7.2 / §11.1
        # Ownership may run past `end` over trailing blank/comment lines, as §11.1 does
        # (DF-C 1211-1673, DF-G 3165-3873).

    def all_starts(self):
        """Flattened, sorted segment starts including loop sub-segment starts."""
        s = set(self.starts)
        for lp in self.loops:
            s.update(lp.starts)
        return sorted(s)

    def segment_of(self, line):
        """Index into all_starts() of the segment containing `line`."""
        starts = self.all_starts()
        idx = -1
        for i, st in enumerate(starts):
            if line >= st:
                idx = i
            else:
                break
        return idx

    def starts_text(self):
        """The 'Segment starts' cell of calc doc §7.1."""
        parts = []
        loops = {lp.header: lp for lp in self.loops}
        for st in self.starts:
            if st in loops:
                lp = loops[st]
                parts.append("[%s %d-%d: %s]" % (lp.label, lp.header, lp.end,
                                                 " · ".join(str(x) for x in lp.starts)))
            else:
                parts.append(str(st))
        return " · ".join(parts)


# calc doc §7.1 (segment starts) and §7.2 (orchestrators and owners) at the pin 16de4b82d.
FUNCTIONS = [
    SplitFunction(
        "offence", "Modules/CalcOffence.lua", "calcs.offence", 348, 6263,
        starts=[348, 1037, 1692, 2174, 2606, 3957, 4155, 4164, 5650],
        loops=[Loop("pass loop", 2606, 3955, [3346]), Loop("ailment loop", 4164, 5648, [4990])],
        units=[("U0", 348, 361, "header + ctx init"), ("OF-A", 362, 1036, "incl. return at 371"),
               ("OF-B", 1037, 1691, ""), ("OF-C", 1692, 2173, ""), ("OF-D", 2174, 2605, ""),
               ("U0", 2606, 2606, "pass loop header"), ("OF-E", 2607, 3345, "pass body A"),
               ("OF-F", 3346, 3955, "pass body B"), ("OF-F", 3956, 4154, "combine passes"),
               ("OF-G", 4155, 4163, "ailment prelude"), ("U0", 4164, 4164, "ailment loop header"),
               ("OF-G", 4165, 4989, "ailment body A"), ("OF-H", 4990, 5648, "ailment body B"),
               ("OF-I", 5649, 6263, "")],
    ),
    SplitFunction(
        "perform", "Modules/CalcPerform.lua", "calcs.perform", 1286, 3990,
        starts=[1286, 1943, 2308, 2997, 3497],
        units=[("U0", 1286, 1288, "header + ctx init"), ("PF-C", 1289, 1942, ""), ("PF-D", 1943, 2307, ""),
               ("PF-E", 2308, 2996, ""), ("PF-F", 2997, 3496, ""), ("PF-G", 3497, 3990, "")],
    ),
    SplitFunction(
        "defence", "Modules/CalcDefence.lua", "calcs.defence", 651, 1671,
        starts=[651, 1211],
        units=[("U0", 651, 658, "header + ctx init"), ("DF-B", 659, 1210, ""), ("DF-C", 1211, 1673, "")],
    ),
    SplitFunction(
        "buildDefenceEstimations", "Modules/CalcDefence.lua", "calcs.buildDefenceEstimations", 1674, 3872,
        starts=[1674, 2153, 2612, 3165],
        units=[("U0", 1674, 1680, "header + ctx init"), ("DF-D", 1681, 2152, ""), ("DF-E", 2153, 2611, ""),
               ("DF-F", 2612, 3164, ""), ("DF-G", 3165, 3873, "")],
    ),
    SplitFunction(
        "initEnv", "Modules/CalcSetup.lua", "calcs.initEnv", 475, 1975,
        starts=[475, 800, 1451],
        units=[("U0", 475, 487, "header, accelerator locals"), ("CS1", 488, 799, ""),
               ("CS2", 800, 1450, "Restart at 1433"), ("CS3", 1451, 1975, "Restart at 1923; final return")],
    ),
]

# Label used for each function's row in calc doc §7.1 (the §7.1 rows give the function's
# full extent; for calcs.defence/buildDefenceEstimations the doc's ranges are 651-1673 and
# 1674-3873, i.e. they include the trailing blank/comment line).
DOC_RANGE = {"offence": (348, 6263), "perform": (1286, 3990), "defence": (651, 1673),
             "buildDefenceEstimations": (1674, 3873), "initEnv": (475, 1975)}


def by_id(fid):
    for f in FUNCTIONS:
        if f.id == fid:
            return f
    raise KeyError(fid)


# ---------------------------------------------------------------------------
# Parsing helpers shared with locals_audit


_CACHE = {}


def load(pob_src, relpath):
    """Parse + resolve a PoB Lua file (cached). Returns (chunk, resolution)."""
    key = os.path.join(pob_src, relpath)
    if key not in _CACHE:
        chunk = luaast.parse_file(key)
        _CACHE[key] = (chunk, luaast.resolve(chunk))
    return _CACHE[key]


def find_function(res, header_line):
    f = luaast.function_name_at(res, header_line)
    if f is None:
        raise LookupError("no function header on line %d" % header_line)
    return f


def find_loop(func, header_line):
    """The loop statement on header_line directly in func's body."""
    for st in func.body.stmts:
        if st.line == header_line and st.kind in ("GenFor", "NumFor", "While", "Repeat"):
            return st
    raise LookupError("no top-level loop on line %d" % header_line)


def check_boundaries(sf, func):
    """Return a list of problems: boundaries that are not statement boundaries."""
    problems = []
    if func.line != sf.header:
        problems.append("%s: header at %d, expected %d" % (sf.name, func.line, sf.header))
    if func.endline != sf.end:
        problems.append("%s: function ends at %d, expected %d" % (sf.name, func.endline, sf.end))

    def straddles(stmts, b, where):
        for st in stmts:
            if st.line < b <= st.endline:
                problems.append("%s: boundary %d splits the statement %d-%d (%s)"
                                % (sf.name, b, st.line, st.endline, where))

    for b in sf.starts[1:]:
        straddles(func.body.stmts, b, "function body")
    for lp in sf.loops:
        try:
            loop = find_loop(func, lp.header)
        except LookupError as e:
            problems.append("%s: %s" % (sf.name, e))
            continue
        if loop.endline != lp.end:
            problems.append("%s: %s ends at %d, expected %d" % (sf.name, lp.label, loop.endline, lp.end))
        for b in lp.splits:
            if not (loop.body.line <= b <= loop.endline):
                problems.append("%s: split %d outside %s" % (sf.name, b, lp.label))
            straddles(loop.body.stmts, b, lp.label + " body")
    # Ownership ranges must tile header..end without gaps or overlaps.
    covered = []
    for unit, a, b, _ in sf.units:
        covered.append((a, b, unit))
    covered.sort()
    expect = sf.header
    for a, b, unit in covered:
        if a != expect:
            problems.append("%s: unit ranges gap/overlap at %d (%s starts at %d)" % (sf.name, expect, unit, a))
        expect = b + 1
    if expect < sf.end + 1:  # the last unit may own trailing blank/comment lines (§11.1)
        problems.append("%s: unit ranges end at %d, function ends at %d" % (sf.name, expect - 1, sf.end))
    return problems


def first_stmt_at_or_after(stmts, line):
    for st in stmts:
        if st.line >= line:
            return st
    return None


# ---------------------------------------------------------------------------
# CLI


def cmd_table(args):
    for sf in FUNCTIONS:
        print("%s  %s:%d-%d" % (sf.name, sf.file, sf.header, sf.end))
        print("  starts: %s" % sf.starts_text())
        for unit, a, b, note in sf.units:
            print("  %-5s %5d-%-5d %s" % (unit, a, b, note))
    return 0


def cmd_check(args):
    rc = 0
    for sf in FUNCTIONS:
        chunk, res = load(args.pob_src, sf.file)
        func = find_function(res, sf.header)
        probs = check_boundaries(sf, func)
        for p in probs:
            print("FAIL " + p)
            rc = 1
        if not probs:
            print("ok   %s: %d segments, %d loop splits" % (sf.name, len(sf.starts), sum(len(l.splits) for l in sf.loops)))
    return rc


def cmd_stages(args):
    import locals_audit  # local import: locals_audit imports this module
    out = []
    out.append("# DRAFT generated by scripts/port/segments.py stages (U0 owns docs/port/stages.toml).")
    out.append("# One probe per segment boundary; `line` is the first statement at/after the boundary,")
    out.append("# the probe is inserted before it. `locals` are the ctx locals declared before the boundary.")
    out.append("")
    for sf in FUNCTIONS:
        chunk, res = load(args.pob_src, sf.file)
        func = find_function(res, sf.header)
        audit = locals_audit.audit_function(sf, chunk, res)
        top = list(audit.params) + list(audit.top)
        loop_rows = audit.loops

        def emit(stage_id, line, decls):
            names = []
            for d in decls:
                if d.line < line and d.name not in names:
                    names.append(d.name)
            out.append("[[stage]]")
            out.append('id = "%s"' % stage_id)
            out.append('file = "%s"' % sf.file)
            out.append("line = %d" % line)
            out.append("locals = [%s]" % ", ".join('"%s"' % n for n in names))
            out.append("")

        loops = {lp.header: lp for lp in sf.loops}
        for st in sf.starts[1:]:
            stmt = first_stmt_at_or_after(func.body.stmts, st)
            emit("%s:%d" % (sf.id, st), stmt.line, top)
        for lp in sf.loops:
            loop = find_loop(func, lp.header)
            ldecls = loop_rows[lp.header]
            first = loop.body.stmts[0]
            emit("%s:%d" % (sf.id, lp.header + 1), first.line, top + ldecls)
            for sp in lp.splits:
                stmt = first_stmt_at_or_after(loop.body.stmts, sp)
                emit("%s:%d" % (sf.id, sp), stmt.line, top + ldecls)
        last = func.body.stmts[-1]
        out_line = last.line if last.kind == "Return" else sf.end
        emit("%s:out" % sf.id, out_line, top)
    text = "\n".join(out)
    if args.out:
        with open(args.out, "w") as fh:
            fh.write(text)
    else:
        print(text)
    return 0


def main(argv=None):
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--pob", default=None, help="PathOfBuilding checkout (contains src/)")
    sub = ap.add_subparsers(dest="cmd", required=True)
    sub.add_parser("table")
    sub.add_parser("check")
    p = sub.add_parser("stages")
    p.add_argument("--out")
    args = ap.parse_args(argv)
    args.pob_src = os.path.join(pobpaths.find_pob(args.pob), "src")
    return {"table": cmd_table, "check": cmd_check, "stages": cmd_stages}[args.cmd](args)


if __name__ == "__main__":
    sys.exit(main())
