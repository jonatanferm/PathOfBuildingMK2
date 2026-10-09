"""Shared helpers for lua2rs: source access, comments, provenance, Rust literals.

Everything here is Python 3 stdlib only (calc doc D19).
"""

import hashlib
import os
import sys

_PORT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
if _PORT not in sys.path:
    sys.path.insert(0, _PORT)

import luaast  # noqa: E402
import pobpaths  # noqa: E402


class Untranslatable(Exception):
    """A construct lua2rs does not translate. `reason` is a short stable slug (it is
    aggregated by the coverage report); `detail` is free text."""

    def __init__(self, reason, detail="", line=None):
        Exception.__init__(self, reason if not detail else "%s: %s" % (reason, detail))
        self.reason = reason
        self.detail = detail
        self.line = line


class LuaSource:
    """One parsed Lua file of the PoB checkout (path relative to src/)."""

    def __init__(self, pob, path):
        self.pob = pob
        self.path = path
        full = os.path.join(pob, "src", path)
        with open(full, "rb") as fh:
            self.data = fh.read()
        if not self.data.strip():
            raise SystemExit("%s is empty: truncated PathOfBuilding checkout? (pass --pob DIR)" % full)
        lines = self.data.split(b"\n")
        if lines and lines[-1] == b"":
            lines.pop()
        self.lines = [ln[:-1] if ln.endswith(b"\r") else ln for ln in lines]
        self.chunk = luaast.parse(self.data, path)
        # Every Name node gets .decl (None for a global): lets the translator tell the file's
        # local aliases (`local mod = modLib.createMod`, `local m_min = math.min`) from globals,
        # and see that a local declared below a list is still a (nil) global inside it.
        self.resolution = luaast.resolve(self.chunk)
        self._comments = None

    def text(self, a, b=None):
        """Source lines a..b (1-based, inclusive) as str (latin-1 round trip)."""
        b = a if b is None else b
        return [ln.decode("latin-1") for ln in self.lines[a - 1:b]]

    def prov_hash(self, a, b):
        """calc doc §3.1 rule 5: sha1 over lines a..b, trailing whitespace stripped, joined by \\n."""
        seg = [ln.rstrip() for ln in self.lines[a - 1:b]]
        return hashlib.sha1(b"\n".join(seg)).hexdigest()[:8]

    def fn_hash(self, a, b):
        """pob-dump FORMAT.md: sha256 over lines a..b exactly as on disk (CR stripped), joined by \\n."""
        return hashlib.sha256(b"\n".join(self.lines[a - 1:b])).hexdigest()

    def marker(self, a, b):
        return "@pob %s:%d-%d h=%s" % (self.path, a, b, self.prov_hash(a, b))

    def comments(self):
        """{line: comment text} for every `--` comment (text after the dashes, stripped)."""
        if self._comments is None:
            self._comments = extract_comments(self.lines)
        return self._comments

    def node_text(self, node):
        return luaast.source_text(self.chunk, node)


def extract_comments(lines):
    """Map line number -> comment text for `--` comments outside string literals.
    Long comments `--[[ ... ]]` are reported on their first line (text up to the line end).
    Long strings spanning lines are tracked so `--` inside them is not a comment."""
    out = {}
    in_long = None  # closing bracket of an open long string/comment
    long_is_comment = False
    for no, raw in enumerate(lines, 1):
        s = raw.decode("latin-1")
        i = 0
        n = len(s)
        if in_long is not None:
            j = s.find(in_long)
            if j < 0:
                if long_is_comment:
                    out[no] = s.strip()
                continue
            if long_is_comment:
                out[no] = s[:j].strip()
            i = j + len(in_long)
            in_long = None
        while i < n:
            c = s[i]
            if c in "\"'":
                q = c
                i += 1
                while i < n and s[i] != q:
                    i += 2 if s[i] == "\\" else 1
                i += 1
                continue
            if c == "[":
                j = i + 1
                while j < n and s[j] == "=":
                    j += 1
                if j < n and s[j] == "[":
                    close = "]" + "=" * (j - i - 1) + "]"
                    k = s.find(close, j + 1)
                    if k < 0:
                        in_long, long_is_comment = close, False
                        break
                    i = k + len(close)
                    continue
            if c == "-" and s.startswith("--", i):
                rest = s[i + 2:]
                if rest.startswith("["):
                    j = 1
                    while j < len(rest) and rest[j] == "=":
                        j += 1
                    if j < len(rest) and rest[j] == "[":
                        close = "]" + "=" * (j - 1) + "]"
                        k = rest.find(close, j + 1)
                        if k < 0:
                            in_long, long_is_comment = close, True
                            out[no] = rest[j + 1:].strip()
                        else:
                            out[no] = rest[j + 1:k].strip()
                        break
                out[no] = rest.strip()
                break
            i += 1
    return out


def find_pob(explicit=None):
    return pobpaths.find_pob(explicit)


PIN_SHORT = pobpaths.PIN_SHORT


# ---------------------------------------------------------------------------
# Rust literals


def rust_str(s):
    """Rust string literal for a Lua string value (luaast gives latin-1-decoded bytes)."""
    try:
        u = s.encode("latin-1").decode("utf-8")
    except (UnicodeDecodeError, UnicodeEncodeError):
        raise Untranslatable("non-utf8-string", repr(s))
    out = ['"']
    for ch in u:
        o = ord(ch)
        if ch == "\\":
            out.append("\\\\")
        elif ch == '"':
            out.append('\\"')
        elif ch == "\n":
            out.append("\\n")
        elif ch == "\t":
            out.append("\\t")
        elif ch == "\r":
            out.append("\\r")
        elif o < 0x20 or o == 0x7F:
            out.append("\\u{%x}" % o)
        else:
            out.append(ch)
    out.append('"')
    return "".join(out)


def rust_f64(v):
    """Rust f64 literal for a Lua number value."""
    if isinstance(v, bool):
        raise Untranslatable("bool-as-number")
    if isinstance(v, int):
        return "%d.0" % v
    if v != v:
        return "f64::NAN"
    if v in (float("inf"), float("-inf")):
        return "f64::INFINITY" if v > 0 else "f64::NEG_INFINITY"
    if v.is_integer() and abs(v) < 1e16:
        return "%d.0" % int(v)
    r = repr(v)
    if "e" in r or "E" in r:
        mant, exp = r.lower().split("e")
        if "." not in mant:
            mant += ".0"
        return "%se%s" % (mant, exp)
    return r


def rust_u32(v):
    if isinstance(v, float):
        if not v.is_integer():
            raise Untranslatable("non-integral-flags", repr(v))
        v = int(v)
    if not (0 <= v < 2 ** 32):
        raise Untranslatable("flags-out-of-range", repr(v))
    return "%d" % v


RUST_KEYWORDS = frozenset(
    "as break const continue crate else enum extern false fn for if impl in let loop match mod move mut "
    "pub ref return self Self static struct super trait true type unsafe use where while async await dyn "
    "abstract become box do final macro override priv typeof unsized virtual yield try gen".split()
)


def rust_ident(name):
    """Lua identifier -> Rust identifier, keeping it verbatim (calc doc §3.1 rule 1)."""
    if name in ("self", "Self", "crate", "super"):
        return name + "_"
    if name in RUST_KEYWORDS:
        return "r#" + name
    return name


def indent(lines, n=1, unit="    "):
    pad = unit * n
    return [(pad + ln) if ln else ln for ln in lines]
