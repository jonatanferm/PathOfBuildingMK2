#!/usr/bin/env python3
"""Measured name-resolution dependency graph of a Rust crate's src/ (CSPLIT-PLAN, CCR-0131).

The resolver of docs/port/calc-split-plan.md §1.1. `calc_depgraph.py graph` runs it on a merged
view of the six calc crates; it also runs on any one src/ tree.

Not a guess from `use` lines: every identifier token of every file is resolved through the
module tree, the file's `use` declarations (explicit, aliased and glob, incl. the prelude glob),
`pub use` re-export chains, `crate::`/`super::`/`self::` paths and `#[macro_export]` macros to the
file that DEFINES the item. Method calls `.m(` are resolved separately when exactly one file of the
crate defines a method `m` in an inherent/trait impl (ambiguous names are counted, not resolved).
`#[cfg(test)]` modules are excluded from the main graph and reported separately.

usage: calc_depgraph_resolve.py <crate>/src <out.json>
"""
import json
import pathlib
import re
import sys
from collections import defaultdict, Counter

SRC = pathlib.Path(sys.argv[1])
OUT = pathlib.Path(sys.argv[2])

# ---------------------------------------------------------------- lexer
ident_re = re.compile(r"[A-Za-z_][A-Za-z0-9_]*")


def lex(text):
    """Tokens (kind, value, line). Comments/strings/chars dropped (strings -> ('str','',line))."""
    toks = []
    i, n, line = 0, len(text), 1
    while i < n:
        c = text[i]
        if c == "\n":
            line += 1; i += 1; continue
        if c.isspace():
            i += 1; continue
        if text.startswith("//", i):
            j = text.find("\n", i)
            i = n if j < 0 else j
            continue
        if text.startswith("/*", i):
            depth, i = 1, i + 2
            while i < n and depth:
                if text.startswith("/*", i): depth += 1; i += 2
                elif text.startswith("*/", i): depth -= 1; i += 2
                else:
                    if text[i] == "\n": line += 1
                    i += 1
            continue
        m = re.match(r'(b|c)?r(#*)"', text[i:i + 300]) if c in "brc" else None
        if m:
            hashes = m.group(2)
            end = '"' + hashes
            j = text.find(end, i + m.end())
            j = n if j < 0 else j + len(end)
            line += text.count("\n", i, j); i = j
            toks.append(("str", "", line)); continue
        if c == '"' or (c in "bc" and i + 1 < n and text[i + 1] == '"'):
            j = i + (2 if c != '"' else 1)
            while j < n and text[j] != '"':
                if text[j] == "\\": j += 1
                j += 1
            j += 1
            line += text.count("\n", i, j); i = j
            toks.append(("str", "", line)); continue
        if c == "'" or (c == "b" and i + 1 < n and text[i + 1] == "'"):
            k = i + (1 if c == "'" else 2)
            # char literal?  'x'  '\n'  '\u{..}'
            m2 = re.match(r"(\\(u\{[0-9a-fA-F]+\}|x[0-9a-fA-F]{2}|.)|[^\\'])'", text[k:k + 12])
            if m2:
                i = k + m2.end(); toks.append(("str", "", line)); continue
            # lifetime
            m3 = ident_re.match(text, k)
            if m3:
                i = m3.end(); toks.append(("life", m3.group(0), line)); continue
            i += 1; continue
        m = ident_re.match(text, i)
        if m:
            v = m.group(0)
            # raw identifiers r#foo
            if v == "r" and text.startswith("#", m.end()) and ident_re.match(text, m.end() + 1):
                m = ident_re.match(text, m.end() + 1); v = m.group(0)
            toks.append(("id", v, line)); i = m.end(); continue
        if c.isdigit():
            m = re.match(r"[0-9][0-9a-zA-Z_.]*", text[i:])
            # don't eat a trailing range `..`
            s = m.group(0)
            if ".." in s: s = s[:s.index("..")]
            toks.append(("num", s, line)); i += len(s); continue
        if text.startswith("::", i):
            toks.append(("p", "::", line)); i += 2; continue
        if text.startswith("->", i) or text.startswith("=>", i):
            toks.append(("p", text[i:i + 2], line)); i += 2; continue
        toks.append(("p", c, line)); i += 1
    return toks


# ---------------------------------------------------------------- module tree
files = sorted(SRC.rglob("*.rs"))


def file_modpath(p):
    rel = p.relative_to(SRC).with_suffix("")
    parts = list(rel.parts)
    if parts == ["lib"]:
        return ()
    if parts[-1] == "mod":
        parts = parts[:-1]
    return tuple(parts)


file_of_mod = {}          # modpath -> file (rel str)  (file-level modules)
mod_of_file = {}
for p in files:
    mp = file_modpath(p)
    rel = str(p.relative_to(SRC))
    file_of_mod[mp] = rel
    mod_of_file[rel] = mp
lines_of = {str(p.relative_to(SRC)): p.read_text().count("\n") for p in files}

# per module (file-level or inline): defs, uses, globs
defs = defaultdict(dict)          # modpath -> name -> (kind, file, line, is_pub)
uses = defaultdict(dict)          # modpath -> name -> (abs path tuple, is_pub)   (explicit/aliased)
globs = defaultdict(list)         # modpath -> [abs path tuple of module]
inline_mod_file = {}              # modpath -> file for inline mods
test_mods = set()
method_defs = defaultdict(set)    # method name -> {(file, selftype)}
impls = []                        # (file, line, selftype, trait or None, in_test)
macro_defs = {}                   # macro name -> file   (#[macro_export] -> crate root namespace)
tokens_of = {}                    # file -> tokens
ctx_of = {}                       # file -> list of (modpath, in_test, in_fn) per token index
crate_pub_crate = Counter()


def norm_path(cur_mod, segs):
    """Turn a path's leading crate/super/self into an absolute module path; None if extern."""
    segs = list(segs)
    if not segs:
        return None
    if segs[0] == "crate":
        return tuple(segs[1:])
    if segs[0] in ("super", "self"):
        base = list(cur_mod)
        while segs and segs[0] in ("super", "self"):
            if segs[0] == "super":
                base = base[:-1]
            segs = segs[1:]
        return tuple(base + segs)
    return ("@rel",) + tuple(segs)   # resolved later (could be a local name / child mod / extern)


def parse_use_tree(toks, i, prefix, out):
    """Parse a use tree at toks[i]; append (path_segs, alias or '*') to out; return index after."""
    segs = list(prefix)
    while i < len(toks):
        k, v, _ = toks[i]
        if k == "id":
            segs.append(v); i += 1
            if i < len(toks) and toks[i][1] == "::":
                i += 1; continue
            if i < len(toks) and toks[i][1] == "as":
                alias = toks[i + 1][1]; i += 2
                out.append((segs, alias)); return i
            out.append((segs, segs[-1])); return i
        if v == "*":
            out.append((segs, "*")); return i + 1
        if v == "{":
            i += 1
            while toks[i][1] != "}":
                j = parse_use_tree(toks, i, segs, out)
                # a token no use tree starts with (`$name` in a macro body): skip it
                i = j if j > i else i + 1
                if toks[i][1] == ",":
                    i += 1
            return i + 1
        if v == "::":
            i += 1; continue
        return i
    return i


ITEM_KW = {"fn", "struct", "enum", "trait", "type", "const", "static", "union"}

for p in files:
    rel = str(p.relative_to(SRC))
    toks = lex(p.read_text())
    tokens_of[rel] = toks
    base_mod = mod_of_file[rel]
    # block stack entries: (kind, modpath, in_test, selftype)
    stack = []
    cur_mod = base_mod
    in_test = False
    pending_attr_test = False
    pending = None      # what the next '{' opens: ('mod', name) / ('impl', selftype, trait) / ('fn',) / ('item',)
    ctx = []
    i = 0
    attr_depth = 0
    while i < len(toks):
        k, v, ln = toks[i]
        in_fn = any(s[0] == "fn" for s in stack)
        ctx.append((cur_mod, in_test, in_fn))
        # attributes: #[cfg(test)]
        if v == "#" and i + 1 < len(toks) and toks[i + 1][1] in ("[", "!"):
            j = i + (2 if toks[i + 1][1] == "[" else 3)
            depth = 1; start = j
            while j < len(toks) and depth:
                if toks[j][1] == "[": depth += 1
                elif toks[j][1] == "]": depth -= 1
                j += 1
            body = [t[1] for t in toks[start:j - 1]]
            if body[:4] == ["cfg", "(", "test", ")"]:
                pending_attr_test = True
            for _ in range(i + 1, j):
                ctx.append((cur_mod, in_test, in_fn))
            i = j
            continue
        if k == "id" and v == "pub" and i + 1 < len(toks) and toks[i + 1][1] == "(":
            if toks[i + 2][1] in ("crate", "super"):
                crate_pub_crate[(rel, toks[i + 2][1])] += 1
        if k == "id" and v == "use" and (i == 0 or toks[i - 1][1] in (";", "{", "}", "]", "pub", ")")):
            is_pub = i > 0 and (toks[i - 1][1] == "pub" or (toks[i - 1][1] == ")" and toks[i - 3][1] == "pub"))
            out = []
            j = parse_use_tree(toks, i + 1, [], out)
            for segs, alias in out:
                ap = norm_path(cur_mod, segs)
                if ap is None: continue
                if alias == "*":
                    globs[cur_mod].append(ap)
                elif alias != "self":
                    uses[cur_mod][alias] = (ap, is_pub)
                else:
                    uses[cur_mod][segs[-2] if len(segs) > 1 else segs[-1]] = (norm_path(cur_mod, segs[:-1]), is_pub)
            for _ in range(i + 1, j):
                ctx.append((cur_mod, in_test, in_fn))
            i = j
            pending_attr_test = False
            continue
        if k == "id" and v == "macro_rules" and toks[i + 1][1] == "!":
            name = toks[i + 2][1]
            # exported iff preceded by #[macro_export] (approx: at file top level, not in fn)
            if not in_fn and not stack:
                macro_defs.setdefault(name, rel)
        if k == "id" and v == "mod" and toks[i + 1][0] == "id" and not in_fn:
            name = toks[i + 1][1]
            if toks[i + 2][1] == "{":
                pending = ("mod", name, pending_attr_test)
            elif toks[i + 2][1] == ";":
                defs[cur_mod][name] = ("mod", rel, ln, True)
                if pending_attr_test:
                    test_mods.add(cur_mod + (name,))
            pending_attr_test = False
        elif k == "id" and v in ITEM_KW and not in_fn and toks[i + 1][0] == "id" and \
                not (v == "const" and toks[i + 1][1] == "fn") and not (v == "type" and stack and stack[-1][0] == "impl"):
            name = toks[i + 1][1]
            in_impl = bool(stack) and stack[-1][0] in ("impl", "trait")
            if in_impl and v == "fn":
                method_defs[name].add((rel, stack[-1][3], in_test or pending_attr_test))
            elif not in_impl:
                is_pub = i > 0 and toks[i - 1][1] in ("pub", ")")
                defs[cur_mod][name] = (v, rel, ln, is_pub)
            if v == "fn":
                pending = ("fn",)
            elif v == "trait":
                pending = ("trait", name)
            else:
                pending = ("item",)
            pending_attr_test = False
        elif k == "id" and v == "const" and toks[i + 1][1] == "fn" and not in_fn:
            pass
        elif k == "id" and v == "impl" and not in_fn and (i == 0 or toks[i - 1][1] in ("}", ";", "]", "{", "unsafe")):
            # impl<...> [Trait for] Type<...> {
            j = i + 1
            if toks[j][1] == "<":
                d = 1; j += 1
                while d:
                    if toks[j][1] == "<": d += 1
                    elif toks[j][1] == ">": d -= 1
                    j += 1
            head = []
            while toks[j][1] != "{":
                head.append(toks[j][1]); j += 1
            trait = None
            if "for" in head:
                fi = head.index("for")
                trait_toks = [t for t in head[:fi] if ident_re.fullmatch(t)]
                trait = trait_toks[-1] if trait_toks else None
                head = head[fi + 1:]
            # self type: first identifier path, last segment before generics
            st = []
            for t in head:
                if t in ("<", "where"): break
                if ident_re.fullmatch(t) and t not in ("dyn", "mut", "const"): st.append(t)
            selftype = st[-1] if st else "?"
            impls.append((rel, ln, selftype, trait, in_test or pending_attr_test, cur_mod, st))
            pending = ("impl", selftype, pending_attr_test)
            pending_attr_test = False
        if v == "{":
            if pending and pending[0] == "mod":
                nm = cur_mod + (pending[1],)
                stack.append(("mod", cur_mod, in_test, None))
                inline_mod_file[nm] = rel
                defs[cur_mod][pending[1]] = ("mod", rel, ln, True)
                if pending[2]:
                    test_mods.add(nm)
                    in_test = True
                cur_mod = nm
            elif pending and pending[0] == "impl":
                stack.append(("impl", cur_mod, in_test, pending[1]))
                if pending[2]: in_test = True
            elif pending and pending[0] == "trait":
                stack.append(("trait", cur_mod, in_test, pending[1]))
            elif pending and pending[0] == "fn":
                stack.append(("fn", cur_mod, in_test, None))
            else:
                stack.append(("blk", cur_mod, in_test, None))
            pending = None
        elif v == "}":
            if stack:
                kind, prev_mod, prev_test, _ = stack.pop()
                cur_mod, in_test = prev_mod, prev_test
        elif v == ";" and pending and pending[0] in ("fn", "item"):
            pending = None
        i += 1
    ctx_of[rel] = ctx

# file-level test modules (mod foo; under cfg(test)) -> whole file is test
test_files = {file_of_mod[m] for m in test_mods if m in file_of_mod}


def module_exists(mp):
    return mp in file_of_mod or mp in inline_mod_file


def find_child(mp, name):
    c = mp + (name,)
    return c if module_exists(c) else None


def resolve(mp, segs, depth=0):
    """Resolve a path (segments relative to module mp, already absolute if mp==()) to
    ('item', defining file, name) / ('mod', modpath) / None (extern)."""
    if depth > 30:
        return None
    segs = list(segs)
    cur = mp
    while segs:
        s = segs[0]
        rest = segs[1:]
        if module_exists(cur + (s,)):
            cur = cur + (s,); segs = rest
            if not segs:
                return ("mod", cur)
            continue
        if s in defs[cur] and defs[cur][s][0] != "mod":
            d = defs[cur][s]
            return ("item", d[1], s, d[0])
        if s in uses[cur]:
            ap, _ = uses[cur][s]
            r = resolve_abs(cur, ap, depth + 1)
            if r is None:
                return None
            if r[0] == "mod":
                cur = r[1]; segs = rest
                if not segs:
                    return r
                continue
            return r   # item (assoc items etc. stay with the type's file)
        # glob imports of cur
        for g in globs[cur]:
            gm = resolve_abs(cur, g, depth + 1)
            if gm and gm[0] == "mod":
                r = resolve(gm[1], segs, depth + 1)
                if r:
                    return r
            elif gm and gm[0] == "item":  # `use Enum::*`
                return gm
        if cur == () and s in macro_defs:
            return ("item", macro_defs[s], s, "macro")
        return None
    return ("mod", cur)


def resolve_abs(cur, ap, depth=0):
    if ap and ap[0] == "@rel":
        segs = ap[1:]
        # 2018: first segment is an extern crate or a name in scope of cur
        r = resolve(cur, segs, depth)
        return r
    return resolve((), ap, depth)


# ------------------------------------------------------------- references per file
STD_METHODS = set("""get set len new clone insert iter push pop remove contains is_empty unwrap expect map
and_then ok_or as_ref as_mut borrow borrow_mut into from to_string to_owned keys values entry extend
first last sort sort_by min max abs floor ceil round sqrt powf powi ln log exp sin cos join split trim
starts_with ends_with find replace chars bytes take skip filter collect fold any all count sum product
retain drain clear default eq cmp partial_cmp hash fmt next rev zip enumerate cloned copied flatten
with_capacity reserve truncate split_off append dedup is_some is_none unwrap_or unwrap_or_else
unwrap_or_default ok err is_ok is_err as_str as_slice parse format write read lock upgrade downgrade
call apply index get_mut iter_mut values_mut contains_key or_insert or_default and or not min_by max_by
with borrow_with position chain step_by windows chunks to_vec as_ptr cast swap replace_range push_str ptr_eq""".split())

refs = defaultdict(Counter)        # (src, dst) -> Counter(symbol)
test_refs = defaultdict(Counter)
method_refs = defaultdict(Counter)
ambig_methods = Counter()
unresolved_crate_paths = Counter()
locs = defaultdict(list)           # (src, dst, sym) -> first lines

method_files = {m: {f for (f, _, t) in s if not t} for m, s in method_defs.items()}

for rel, toks in tokens_of.items():
    ctx = ctx_of[rel]
    file_is_test = rel in test_files
    i = 0
    n = len(toks)
    while i < n:
        k, v, ln = toks[i]
        if k != "id":
            i += 1; continue
        cur_mod, in_test, in_fn = ctx[i] if i < len(ctx) else ctx[-1]
        is_test = in_test or file_is_test
        prev = toks[i - 1][1] if i else ""
        if prev == "::" or prev in ("fn", "struct", "enum", "trait", "type", "const", "static", "mod", "union", "use", "macro_rules", "let", "'"):
            i += 1; continue
        if prev == ".":
            if i + 1 < n and (toks[i + 1][1] == "(" or toks[i + 1][1] == "::"):
                fs = method_files.get(v)
                if fs and v not in STD_METHODS:
                    if len(fs) == 1:
                        dst = next(iter(fs))
                        if dst != rel:
                            (test_refs if is_test else method_refs)[(rel, dst)][v] += 1
                            if not is_test and len(locs[(rel, dst, v)]) < 4: locs[(rel, dst, v)].append(ln)
                    else:
                        ambig_methods[v] += 1
            i += 1; continue
        # collect path a::b::c
        segs = [v]; j = i + 1
        while j + 1 < n and toks[j][1] == "::" and toks[j + 1][0] == "id":
            segs.append(toks[j + 1][1]); j += 2
        while j + 1 < n and toks[j][1] == "::" and toks[j + 1][1] == "<":   # turbofish ::<..>::
            break
        is_macro = j < n and toks[j][1] == "!" and len(segs) == 1 and (j + 1 < n and toks[j + 1][1] in ("(", "[", "{"))
        r = None
        if segs[0] in ("crate", "super", "self"):
            ap = norm_path(cur_mod, segs)
            r = resolve((), ap)
            if r is None and segs[0] == "crate":
                unresolved_crate_paths[".".join(segs)] += 1
        elif segs[0] == "Self":
            r = None
        elif is_macro:
            if segs[0] in macro_defs:
                r = ("item", macro_defs[segs[0]], segs[0], "macro")
        else:
            # local name in scope of cur_mod (defs, explicit uses, globs); locals/params can shadow
            # a glob name (we cannot see let-bindings' types), so only resolve when the token is
            # used as a path head, a type, a call or a struct literal is not required: count all.
            r = resolve(cur_mod, segs)
            if r is None and len(segs) > 1:
                r = resolve(cur_mod, segs[:1])
        if r and r[0] == "item":
            dst = r[1]
            if dst != rel:
                (test_refs if is_test else refs)[(rel, dst)][r[2]] += 1
                if not is_test and len(locs[(rel, dst, r[2])]) < 4: locs[(rel, dst, r[2])].append(ln)
        elif r and r[0] == "mod":
            pass
        i = j if len(segs) > 1 else i + 1

# impls on foreign (other-file) types: inherent impls must live in the type's crate
type_def_file = {}
for mp, d in defs.items():
    for name, (kind, f, ln, pub) in d.items():
        if kind in ("struct", "enum", "type", "union", "trait"):
            type_def_file.setdefault(name, set()).add(f)
foreign_impls = []
for (f, ln, st, trait, t, mp, path) in impls:
    if t or f in test_files: continue
    r = resolve(mp, path) if path else None
    dfile = r[1] if r and r[0] == "item" else None
    if dfile is None:
        # unresolved path: fall back to the unique type name
        cands = type_def_file.get(st, set())
        dfile = next(iter(cands)) if len(cands) == 1 else None
    tr = resolve(mp, [trait]) if trait else None
    tfile = tr[1] if tr and tr[0] == "item" else None
    if dfile and dfile != f:
        foreign_impls.append({"file": f, "line": ln, "self": st, "self_file": dfile, "trait": trait,
                              "trait_file": tfile})


def ser(d):
    return [{"src": a, "dst": b, "n": sum(c.values()), "syms": c.most_common()} for (a, b), c in sorted(d.items())]


json.dump({
    "lines": lines_of,
    "test_files": sorted(test_files),
    "refs": ser(refs),
    "method_refs": ser(method_refs),
    "test_refs": ser(test_refs),
    "ambiguous_methods": ambig_methods.most_common(60),
    "foreign_impls": foreign_impls,
    "macros": macro_defs,
    "pub_restricted": [{"file": f, "kind": k, "n": c} for (f, k), c in sorted(crate_pub_crate.items())],
    "unresolved_crate_paths": unresolved_crate_paths.most_common(40),
    "locs": [[a, b, c, l] for (a, b, c), l in sorted(locs.items())],
}, OUT.open("w"), indent=1)
print("files", len(files), "edges", len(refs), "method edges", len(method_refs), "foreign impls", len(foreign_impls))
print("unresolved crate:: paths", sum(unresolved_crate_paths.values()), unresolved_crate_paths.most_common(10))
