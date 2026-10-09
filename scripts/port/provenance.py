#!/usr/bin/env python3
"""Provenance markers and upstream sync for the Rust port (calc doc §3.1 rule 5, §9.6).

Markers (in `//`, `///` or `//!` comments of .rs files):
  //! @pob-file <path> @<sha>                      file header: the Lua file and the pin
  /// @pob <path>:<a>-<b> h=<8hex>                  a ported fn or segment
  // @pob-skip <path>:<a>-<b> reason="..."           a deliberate exclusion
<path> is relative to PathOfBuilding/src (`runtime/lua/xml.lua` / `../runtime/lua/xml.lua`
name files outside src). The hash is the first 8 hex digits of SHA-1 over lines a..b,
each with trailing whitespace stripped, joined with "\\n".

Commands:
  check     [RS_PATHS...] [--pin SHA]
                            verify every @pob hash and @pob-file/@pob-skip against the checkout;
                            @pob-file pins must name the checkout's HEAD (or --pin)
  coverage  [RS_PATHS...]   report in-scope Lua lines (docs/port/scope.toml) that no marker or
                            @pob-skip covers, or that two cover. Blank and comment-only lines
                            are ignored unless --strict. --range PATH:A-B restricts the scope.
  bump --to SHA [--from SHA] [RS_PATHS...] [--apply] [--report FILE]
                            remap every range through `git diff -U0 FROM SHA` hunks (FROM
                            defaults to the checkout's HEAD); markers whose hash is unchanged
                            get new line numbers (written with --apply, together with the
                            @pob-file pin when the whole file is clean); every other range is
                            listed with its Lua diff as a work item.

Upgrade workflow: with the checkout at the old pin, run `bump --to NEW --apply`; then check
out NEW in the PathOfBuilding checkout. `check` now passes for every bumped file and fails for
the work items' files (their pins still name the old commit) until they are re-ported and
their @pob-file pins edited. When no work items remain, set pobpaths.PIN to NEW.
  marker PATH:A-B           print the @pob marker (with its hash) for a range
  scope --emit              print the default scope.toml (calc doc §11.1) for U0

RS_PATHS default to crates/ of the repository. --pob DIR selects the PathOfBuilding checkout.
"""

import argparse
import hashlib
import os
import re
import subprocess
import sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))

import calcscope  # noqa: E402
import luaast  # noqa: E402
import pobpaths  # noqa: E402

_COMMENT = r"//[/!]?\s*"
RE_FILE = re.compile(_COMMENT + r"@pob-file\s+(?P<path>\S+)\s+@(?P<sha>[0-9a-fA-F]+)")
RE_POB = re.compile(_COMMENT + r"@pob\s+(?P<path>[^\s:]+):(?P<a>\d+)(?:-(?P<b>\d+))?(?:\s+h=(?P<h>[0-9a-fA-F]+))?")
RE_SKIP = re.compile(_COMMENT + r"@pob-skip\s+(?P<path>[^\s:]+):(?P<a>\d+)(?:-(?P<b>\d+))?(?:\s+reason=\"(?P<reason>[^\"]*)\")?")


class Marker:
    def __init__(self, kind, rs, rsline, path, a, b, h=None, sha=None, reason=None, span=None):
        self.kind = kind      # 'pob' | 'skip' | 'file'
        self.rs = rs          # Rust file path
        self.rsline = rsline  # 1-based line in the Rust file
        self.path = path      # canonical Lua path (relative to src/, "../runtime/..." outside)
        self.a = a
        self.b = b
        self.h = h
        self.sha = sha
        self.reason = reason
        self.span = span      # (start, end) of the match within the Rust line

    def where(self):
        return "%s:%d" % (self.rs, self.rsline)

    def range_text(self):
        return "%s:%d-%d" % (self.path, self.a, self.b)


def canon_path(p):
    p = p.replace("\\", "/")
    if p.startswith("src/"):
        p = p[4:]
    if p.startswith("runtime/"):
        p = "../" + p
    return p


def lua_file(pob, path):
    return os.path.normpath(os.path.join(pob, "src", path))


# ---------------------------------------------------------------------------
# Hashing


def split_lines(data):
    lines = data.split(b"\n")
    if lines and lines[-1] == b"":
        lines.pop()
    return lines


def range_hash_lines(lines, a, b):
    seg = [ln.rstrip() for ln in lines[a - 1:b]]
    return hashlib.sha1(b"\n".join(seg)).hexdigest()[:8]


_FILE_CACHE = {}


def read_lua(pob, path):
    full = lua_file(pob, path)
    if full not in _FILE_CACHE:
        with open(full, "rb") as fh:
            _FILE_CACHE[full] = split_lines(fh.read())
    return _FILE_CACHE[full]


def range_hash(pob, path, a, b):
    return range_hash_lines(read_lua(pob, path), a, b)


# ---------------------------------------------------------------------------
# Marker scanning


def rust_files(paths):
    out = []
    for p in paths:
        if os.path.isfile(p):
            out.append(p)
            continue
        for dirpath, dirnames, filenames in os.walk(p):
            dirnames[:] = sorted(d for d in dirnames if d not in ("target", ".git", "node_modules"))
            for fn in sorted(filenames):
                if fn.endswith(".rs"):
                    out.append(os.path.join(dirpath, fn))
    return out


def scan_markers(files):
    markers = []
    for rs in files:
        with open(rs, encoding="utf-8", errors="replace") as fh:
            for i, line in enumerate(fh, 1):
                if "@pob" not in line:
                    continue
                for m in RE_FILE.finditer(line):
                    markers.append(Marker("file", rs, i, canon_path(m.group("path")), 0, 0, sha=m.group("sha"),
                                          span=m.span()))
                for m in RE_POB.finditer(line):
                    a = int(m.group("a"))
                    b = int(m.group("b") or a)
                    markers.append(Marker("pob", rs, i, canon_path(m.group("path")), a, b,
                                          h=(m.group("h") or None), span=m.span()))
                for m in RE_SKIP.finditer(line):
                    a = int(m.group("a"))
                    b = int(m.group("b") or a)
                    markers.append(Marker("skip", rs, i, canon_path(m.group("path")), a, b,
                                          reason=m.group("reason"), span=m.span()))
    return markers


# ---------------------------------------------------------------------------
# check


def cmd_check(args, markers):
    errors = 0
    counts = {"file": 0, "pob": 0, "skip": 0}
    # Hashes are verified against the files on disk, so pins are compared with the commit the
    # checkout has checked out (not with a hard-coded constant that a bump leaves stale).
    pin = (args.pin or pobpaths.checkout_pin(args.pob)).lower()
    if not args.pin and pin != pobpaths.PIN:
        print("note: checkout is at %s, pobpaths.PIN is %s (update PIN once the whole port is bumped)" % (
            pin[:9], pobpaths.PIN_SHORT))
    for m in markers:
        counts[m.kind] += 1
        full = lua_file(args.pob, m.path)
        if not os.path.isfile(full):
            print("FAIL %s: Lua file %s not found" % (m.where(), m.path))
            errors += 1
            continue
        if m.kind == "file":
            if not pin.startswith(m.sha.lower()) or len(m.sha) < 7:
                print("FAIL %s: @pob-file pin @%s is not the pin %s (the checkout's HEAD)" % (
                    m.where(), m.sha, pin[:9]))
                errors += 1
            continue
        n = len(read_lua(args.pob, m.path))
        if not (1 <= m.a <= m.b <= n):
            print("FAIL %s: range %s outside the file (1-%d)" % (m.where(), m.range_text(), n))
            errors += 1
            continue
        if m.kind == "skip":
            if not m.reason:
                print("FAIL %s: @pob-skip %s without reason=\"...\"" % (m.where(), m.range_text()))
                errors += 1
            continue
        h = range_hash(args.pob, m.path, m.a, m.b)
        if m.h is None:
            print("FAIL %s: @pob %s has no hash (expected h=%s)" % (m.where(), m.range_text(), h))
            errors += 1
        elif m.h.lower() != h:
            print("FAIL %s: @pob %s h=%s, Lua hashes to %s" % (m.where(), m.range_text(), m.h, h))
            errors += 1
    print("%s: %d @pob, %d @pob-skip, %d @pob-file markers, %d error(s)" % (
        "ok" if errors == 0 else "FAIL", counts["pob"], counts["skip"], counts["file"], errors))
    return 1 if errors else 0


# ---------------------------------------------------------------------------
# coverage


_CODE_LINES = {}


def code_lines(pob, path):
    """Lines holding at least one Lua token (blank and comment-only lines excluded)."""
    full = lua_file(pob, path)
    if full not in _CODE_LINES:
        with open(full, "rb") as fh:
            toks = luaast.tokenize(fh.read())
        s = set()
        for t in toks:
            if t.kind == "eof":
                continue
            s.update(range(t.line, t.endline + 1))
        _CODE_LINES[full] = s
    return _CODE_LINES[full]


def to_ranges(lines):
    out = []
    for ln in sorted(lines):
        if out and ln == out[-1][1] + 1:
            out[-1][1] = ln
        else:
            out.append([ln, ln])
    return out


def fmt_ranges(rs, limit=12):
    parts = ["%d-%d" % (a, b) if a != b else "%d" % a for a, b in rs[:limit]]
    if len(rs) > limit:
        parts.append("... (%d more)" % (len(rs) - limit))
    return ", ".join(parts)


def parse_range_arg(s):
    path, _, rng = s.rpartition(":")
    a, _, b = rng.partition("-")
    return canon_path(path), int(a), int(b or a)


def cmd_coverage(args, markers):
    if args.range:
        entries = {}
        for r in args.range:
            p, a, b = parse_range_arg(r)
            entries.setdefault(p, []).append((a, b))
        scope = calcscope.Scope([(p, rs, "") for p, rs in entries.items()], "--range")
    else:
        scope_path = args.scope or os.path.join(pobpaths.docs_port(), "scope.toml")
        if not os.path.exists(scope_path) and not args.default_scope:
            print("FAIL scope file %s not found (U0 owns docs/port/scope.toml); pass --scope, --range or "
                  "--default-scope" % os.path.relpath(scope_path))
            return 2
        scope = calcscope.load_scope(scope_path if os.path.exists(scope_path) else None,
                                     calcscope.default_closures_json(pobpaths.main_checkout()))
    cover = {}
    for m in markers:
        if m.kind in ("pob", "skip"):
            for ln in range(m.a, m.b + 1):
                cover.setdefault((m.path, ln), []).append(m)
    total = uncovered_n = double_n = 0
    bad = False
    for path in scope.files():
        if not os.path.isfile(lua_file(args.pob, path)):
            print("FAIL %s: in scope but not in the checkout" % path)
            bad = True
            continue
        n = len(read_lua(args.pob, path))
        lines = set()
        for a, b in scope.ranges(path):
            lines.update(range(a, min(b, n) + 1))
        if not args.strict:
            lines &= code_lines(args.pob, path)
        unc = [ln for ln in lines if (path, ln) not in cover]
        dbl = [ln for ln in lines if len(cover.get((path, ln), ())) > 1]
        total += len(lines)
        uncovered_n += len(unc)
        double_n += len(dbl)
        if unc or dbl or args.verbose:
            print("%s: %d in-scope lines, %d uncovered, %d covered twice" % (path, len(lines), len(unc), len(dbl)))
            if unc:
                print("  uncovered: %s" % fmt_ranges(to_ranges(unc)))
            if dbl:
                print("  covered twice: %s" % fmt_ranges(to_ranges(dbl)))
                for ln in sorted(dbl)[:3]:
                    print("    line %d: %s" % (ln, ", ".join(m.where() for m in cover[(path, ln)])))
    pct = 100.0 * (total - uncovered_n) / total if total else 100.0
    ok = uncovered_n == 0 and double_n == 0 and not bad
    print("%s: scope %s; %d in-scope lines, %d uncovered, %d covered twice (coverage %.2f%%)" % (
        "ok" if ok else "FAIL", scope.source, total, uncovered_n, double_n, pct))
    return 0 if ok else 1


# ---------------------------------------------------------------------------
# bump


class Hunk:
    def __init__(self, a, b, c, d, text):
        self.a, self.b, self.c, self.d = a, b, c, d  # old start/count, new start/count
        self.text = text

    @property
    def old_end(self):
        return self.a + self.b - 1

    def touches(self, x, y):
        if self.b == 0:            # pure insertion after old line a
            return x <= self.a < y
        return not (self.old_end < x or self.a > y)


def git(pob, *argv):
    return subprocess.run(["git", "-C", pob] + list(argv), capture_output=True, check=True).stdout


def parse_hunks(diff):
    hunks = []
    cur = None
    for raw in diff.split(b"\n"):
        line = raw.decode("latin-1")
        m = re.match(r"^@@ -(\d+)(?:,(\d+))? \+(\d+)(?:,(\d+))? @@", line)
        if m:
            a = int(m.group(1))
            b = int(m.group(2)) if m.group(2) is not None else 1
            c = int(m.group(3))
            d = int(m.group(4)) if m.group(4) is not None else 1
            cur = Hunk(a, b, c, d, [line])
            hunks.append(cur)
        elif cur is not None and (line.startswith("+") or line.startswith("-") or line.startswith("\\")):
            if not (line.startswith("+++") or line.startswith("---")):
                cur.text.append(line)
    return hunks


def map_line(hunks, x, end=False):
    """Map old line x to the new file. Lines inside a changed hunk map to the hunk's
    new start (or new end when `end`)."""
    delta = 0
    for h in hunks:
        if h.b == 0:
            if h.a < x:
                delta += h.d
            continue
        if h.old_end < x:
            delta += h.d - h.b
        elif h.a <= x <= h.old_end:
            if end:
                return h.c + max(h.d, 1) - 1
            return h.c if h.d else h.c + 1
    return x + delta


def cmd_bump(args, markers):
    pob = args.pob
    frm = git(pob, "rev-parse", args.frm or pobpaths.checkout_pin(pob)).decode().strip()
    to = git(pob, "rev-parse", args.to).decode().strip()
    to_short = to[:9]
    if frm == to:
        print("bump: --from and --to are both %s; run bump while the checkout is still at the old pin, "
              "or pass --from <old pin>" % to_short)
        return 2
    lua_paths = sorted({m.path for m in markers if m.kind in ("pob", "skip", "file")})
    hunks_by_path = {}
    new_lines = {}
    old_lines = {}
    missing = set()
    for p in lua_paths:
        gp = os.path.normpath(os.path.join("src", p)).replace(os.sep, "/")
        try:
            old_lines[p] = split_lines(git(pob, "show", "%s:%s" % (frm, gp)))
        except subprocess.CalledProcessError:
            missing.add(p)
            continue
        try:
            new_lines[p] = split_lines(git(pob, "show", "%s:%s" % (to, gp)))
        except subprocess.CalledProcessError:
            new_lines[p] = None
        diff = git(pob, "diff", "-U0", "--no-color", "--no-ext-diff", frm, to, "--", gp)
        hunks_by_path[p] = parse_hunks(diff)

    rewrites = {}   # rs -> list of (rsline, span, old_text, new_text)
    work = []       # (marker, new_a, new_b, reason, hunks)
    clean = unchanged = 0
    dirty_rs = set()
    for m in markers:
        if m.kind == "file":
            continue
        if m.path in missing:
            work.append((m, None, None, "Lua file missing at %s" % frm[:9], []))
            dirty_rs.add(m.rs)
            continue
        if new_lines.get(m.path) is None:
            work.append((m, None, None, "Lua file deleted at %s" % to_short, []))
            dirty_rs.add(m.rs)
            continue
        hunks = hunks_by_path[m.path]
        na = map_line(hunks, m.a)
        nb = map_line(hunks, m.b, end=True)
        touching = [h for h in hunks if h.touches(m.a, m.b)]
        old_h = m.h.lower() if (m.kind == "pob" and m.h) else range_hash_lines(old_lines[m.path], m.a, m.b)
        new_h = range_hash_lines(new_lines[m.path], na, nb) if 1 <= na <= nb <= len(new_lines[m.path]) else None
        if new_h == old_h:
            clean += 1
            if (na, nb) == (m.a, m.b):
                unchanged += 1
            else:
                rewrites.setdefault(m.rs, []).append((m, na, nb))
        else:
            dirty_rs.add(m.rs)
            work.append((m, na, nb, "Lua changed (hash %s -> %s)" % (old_h, new_h or "n/a"), touching))

    # Apply / report.
    out = []
    out.append("# provenance bump %s -> %s" % (frm[:9], to_short))
    out.append("")
    out.append("- markers: %d clean (%d moved, %d in place), %d work items" % (
        clean, sum(len(v) for v in rewrites.values()), unchanged, len(work)))
    for rs, items in sorted(rewrites.items()):
        for m, na, nb in items:
            out.append("- move %s: %s -> %d-%d" % (m.where(), m.range_text(), na, nb))
    out.append("")
    out.append("## Work items")
    for m, na, nb, why, hunks in work:
        out.append("")
        out.append("### %s `%s` (%s)" % (m.where(), m.range_text(), "@pob-skip" if m.kind == "skip" else "@pob"))
        out.append("")
        out.append("%s; new range ~ %s" % (why, ("%d-%d" % (na, nb)) if na else "?"))
        if hunks:
            out.append("")
            out.append("```diff")
            for h in hunks:
                out.extend(h.text)
            out.append("```")
    report = "\n".join(out) + "\n"
    if args.report:
        with open(args.report, "w") as fh:
            fh.write(report)
    print(report, end="")

    if args.apply:
        files = set(rewrites) | {m.rs for m in markers if m.kind == "file"}
        for rs in sorted(files):
            with open(rs, encoding="utf-8") as fh:
                lines = fh.read().split("\n")
            for m, na, nb in rewrites.get(rs, []):
                ln = lines[m.rsline - 1]
                old = "%s:%d-%d" % (_orig_path_text(ln, m), m.a, m.b)
                new = "%s:%d-%d" % (_orig_path_text(ln, m), na, nb)
                if old not in ln:  # single-line form `path:a`
                    old = "%s:%d" % (_orig_path_text(ln, m), m.a)
                lines[m.rsline - 1] = ln.replace(old, new, 1)
            if rs not in dirty_rs:
                for m in markers:
                    if m.kind == "file" and m.rs == rs:
                        ln = lines[m.rsline - 1]
                        lines[m.rsline - 1] = ln.replace("@" + m.sha, "@" + to_short, 1)
            with open(rs, "w", encoding="utf-8") as fh:
                fh.write("\n".join(lines))
        print("applied: %d marker(s) moved in %d file(s); @pob-file pins bumped in %d clean file(s)" % (
            sum(len(v) for v in rewrites.values()), len(rewrites),
            len({m.rs for m in markers if m.kind == "file"} - dirty_rs)))
    return 1 if work else 0


def _orig_path_text(line, m):
    seg = line[m.span[0]:m.span[1]]
    mm = re.search(r"@pob(?:-skip)?\s+([^\s:]+):", seg)
    return mm.group(1)


# ---------------------------------------------------------------------------


def main(argv=None):
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--pob", help="PathOfBuilding checkout (default: see pobpaths.find_pob)")
    sub = ap.add_subparsers(dest="cmd", required=True)
    p = sub.add_parser("check")
    p.add_argument("paths", nargs="*")
    p.add_argument("--pin", help="expected @pob-file pin (default: the checkout's HEAD)")
    p = sub.add_parser("coverage")
    p.add_argument("paths", nargs="*")
    p.add_argument("--scope")
    p.add_argument("--default-scope", action="store_true", help="use the built-in scope when scope.toml is missing")
    p.add_argument("--range", action="append", help="restrict the scope to PATH:A-B (repeatable)")
    p.add_argument("--strict", action="store_true", help="count blank and comment-only lines too")
    p.add_argument("--verbose", action="store_true")
    p = sub.add_parser("bump")
    p.add_argument("paths", nargs="*")
    p.add_argument("--to", required=True)
    p.add_argument("--from", dest="frm")
    p.add_argument("--apply", action="store_true")
    p.add_argument("--report")
    p = sub.add_parser("marker")
    p.add_argument("range")
    p = sub.add_parser("scope")
    p.add_argument("--emit", action="store_true", required=True)
    args = ap.parse_args(argv)
    args.pob = pobpaths.find_pob(args.pob)

    if args.cmd == "marker":
        path, a, b = parse_range_arg(args.range)
        print("/// @pob %s:%d-%d h=%s" % (path, a, b, range_hash(args.pob, path, a, b)))
        return 0
    if args.cmd == "scope":
        sc = calcscope.default_scope(calcscope.default_closures_json(pobpaths.main_checkout()))
        print(sc.to_toml(pobpaths.PIN))
        return 0
    paths = args.paths or [os.path.join(pobpaths.repo_root(), "crates")]
    markers = scan_markers(rust_files(paths))
    if args.cmd == "check":
        return cmd_check(args, markers)
    if args.cmd == "coverage":
        return cmd_coverage(args, markers)
    if args.cmd == "bump":
        return cmd_bump(args, markers)
    return 2


if __name__ == "__main__":
    sys.exit(main())
