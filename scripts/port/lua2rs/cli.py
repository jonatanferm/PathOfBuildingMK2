"""lua2rs command line.

  modparser --list NAME [--range A-B] [--fn NAME] [-o OUT]   one ModParser pattern list
  config [--range A-B] [-o OUT]                              ConfigOptions apply closures
  coverage [--json OUT]                                      translation coverage of every input
  check [--no-cargo]                                         generate everything (plus the golden
                                                             fixtures) into the stub crate
                                                             lua2rs/check and `cargo check` it

Common options: --pob DIR (PathOfBuilding checkout), --shape FILE.json (output shape override).
"""

import argparse
import json
import os
import re
import subprocess
import sys

from . import configopts, modparser
from .common import LuaSource, find_pob
from .shape import load_shape

HERE = os.path.dirname(os.path.abspath(__file__))
PORT = os.path.dirname(HERE)
REPO = os.path.dirname(os.path.dirname(PORT))
CHECK_DIR = os.path.join(HERE, "check")


def load_flag_names(repo=REPO):
    """ModFlag / KeywordFlag / SkillType names from P1's generated Data/Global.lua enums."""
    path = os.path.join(repo, "crates", "pob-dump", "generated", "global.rs")
    if not os.path.exists(path):
        return {}
    with open(path, encoding="utf-8") as fh:
        text = fh.read()
    out = {}
    for mod in ("ModFlag", "KeywordFlag", "SkillType"):
        m = re.search(r"pub mod %s \{(.*?)\n\}" % mod, text, re.S)
        if m:
            out[mod] = set(re.findall(r"pub const (\w+): (?:u32|T) =", m.group(1)))
    return out


def parse_range(s):
    if not s:
        return None
    a, _, b = s.partition("-")
    return (int(a), int(b or a))


def _write(path, text):
    if path in (None, "-"):
        sys.stdout.write(text)
        return
    d = os.path.dirname(os.path.abspath(path))
    os.makedirs(d, exist_ok=True)
    with open(path, "w", encoding="utf-8") as fh:
        fh.write(text)


def gen_list(pob, shape, name, rng=None, fn=None):
    src = LuaSource(pob, modparser.MODPARSER)
    g = modparser.ListGen(src, shape, load_flag_names())
    return g.generate(name, rng, fn)


def gen_config(pob, shape, rng=None):
    src = LuaSource(pob, configopts.CONFIG)
    g = configopts.ConfigGen(src, shape, load_flag_names())
    return g.generate(rng)


def coverage(pob, shape):
    rep = {"modparser": {}, "config": None}
    for name in modparser.LISTS:
        _, entries = gen_list(pob, shape, name)
        rep["modparser"][name] = modparser.stats(entries)
    _, inline, shared = gen_config(pob, shape)
    rep["config"] = configopts.stats(inline, shared)
    return rep


def pct(ok, total):
    return "%.1f%%" % (100.0 * ok / total) if total else "n/a"


def print_coverage(rep, out=sys.stdout):
    w = out.write
    w("ModParser lists (duplicates excluded):\n")
    w("  %-15s %13s %13s %13s %13s %13s\n" % ("list", "static", "fn-oneline", "fn-singleret", "fn-multi", "all"))
    for name, st in rep["modparser"].items():
        cells = []
        for cat in ("static", "fn-oneline", "fn-singleret", "fn-multi", "all"):
            c = st[cat]
            cells.append("%d/%d" % (c["ok"], c["total"]) if c["total"] else "-")
        w("  %-15s %13s %13s %13s %13s %13s\n" % tuple([name] + cells))
    sp = rep["modparser"]["specialModList"]
    so = sp["static+oneline"]
    w("specialModList static + one-line closures: %d/%d = %s (gate >= 95%%)\n" % (so["ok"], so["total"], pct(so["ok"], so["total"])))
    sr = {"ok": so["ok"] + sp["fn-singleret"]["ok"], "total": so["total"] + sp["fn-singleret"]["total"]}
    w("specialModList static + single-return closures: %d/%d = %s\n" % (sr["ok"], sr["total"], pct(sr["ok"], sr["total"])))
    for name in ("formList", "modNameList", "modFlagList"):
        a = rep["modparser"][name]["all"]
        w("%s: %d/%d = %s (gate: all translate)\n" % (name, a["ok"], a["total"], pct(a["ok"], a["total"])))
    c = rep["config"]
    w("ConfigOptions inline apply closures: %d/%d = %s (gate >= 80%%); %d options use the shared mapAffixDropDownFunction\n"
      % (c["inline"]["ok"], c["inline"]["total"], pct(c["inline"]["ok"], c["inline"]["total"]), c["shared_fn_options"]))
    w("Untranslated reasons:\n")
    for name, st in rep["modparser"].items():
        if st["fail_reasons"]:
            w("  %s: %s\n" % (name, ", ".join("%s %d" % kv for kv in st["fail_reasons"].items())))
    if c["fail_reasons"]:
        w("  ConfigOptions: %s\n" % ", ".join("%s %d" % kv for kv in c["fail_reasons"].items()))


CHECK_PRELUDE = {
    "modparser": [
        "#![allow(non_snake_case, non_upper_case_globals, unused_variables, unused_mut, unused_parens, unused_imports, clippy::all)]",
        "use crate::prelude::*;",
    ],
    "config": [
        "#![allow(non_snake_case, non_upper_case_globals, unused_variables, unused_mut, unused_parens, unused_imports, clippy::all)]",
        "use crate::prelude::*;",
        "use crate::calc::*;",
    ],
}


def run_check(pob, shape_path, keep=False, cargo=True):
    shape = load_shape(shape_path, {"prelude": CHECK_PRELUDE})
    gen = os.path.join(CHECK_DIR, "src", "gen")
    os.makedirs(gen, exist_ok=True)
    mods = []
    for name in modparser.LISTS:
        text, _ = gen_list(pob, shape, name)
        fn = "%s.rs" % name
        _write(os.path.join(gen, fn), text)
        mods.append(name)
    text, _, _ = gen_config(pob, shape)
    _write(os.path.join(gen, "config_apply.rs"), text)
    mods.append("config_apply")
    # the golden-test fixtures (tests/fixtures/pob) must compile too
    fixture = os.path.join(HERE, "tests", "fixtures", "pob")
    for name in modparser.LISTS:
        text, _ = gen_list(fixture, shape, name)
        _write(os.path.join(gen, "fixture_%s.rs" % name), text)
        mods.append("fixture_%s" % name)
    text, _, _ = gen_config(fixture, shape)
    _write(os.path.join(gen, "fixture_config_apply.rs"), text)
    mods.append("fixture_config_apply")
    modrs = ["//! GENERATED by `lua2rs check` (gitignored)."] + ["pub mod %s;" % m for m in mods]
    _write(os.path.join(gen, "mod.rs"), "\n".join(modrs) + "\n")
    if not cargo:
        return 0
    cargo_sh = os.path.join(PORT, "cargo.sh")
    cmd = [cargo_sh, "check", "--manifest-path", os.path.join(CHECK_DIR, "Cargo.toml"), "--offline", "--message-format", "short"]
    print("+ " + " ".join(cmd), file=sys.stderr)
    rc = subprocess.call(cmd)
    return rc


def main(argv=None):
    ap = argparse.ArgumentParser(prog="lua2rs", description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--pob", help="PathOfBuilding checkout (default: pobpaths.find_pob)")
    ap.add_argument("--shape", help="JSON file merged over the default output shape")
    sub = ap.add_subparsers(dest="cmd", required=True)
    p = sub.add_parser("modparser")
    p.add_argument("--list", required=True, choices=modparser.LISTS)
    p.add_argument("--range", help="Lua line range A-B (entries whose key is on these lines)")
    p.add_argument("--fn", help="name of the pub push fn for a partial range")
    p.add_argument("-o", "--out")
    p = sub.add_parser("config")
    p.add_argument("--range", help="Lua line range A-B of ConfigOptions.lua")
    p.add_argument("-o", "--out")
    p = sub.add_parser("coverage")
    p.add_argument("--json")
    p = sub.add_parser("check")
    p.add_argument("--no-cargo", action="store_true", help="only (re)generate the stub crate sources")
    args = ap.parse_args(argv)
    pob = find_pob(args.pob)
    if args.cmd == "check":
        return run_check(pob, args.shape, cargo=not args.no_cargo)
    shape = load_shape(args.shape)
    if args.cmd == "modparser":
        text, entries = gen_list(pob, shape, args.list, parse_range(args.range), args.fn)
        _write(args.out, text)
        st = modparser.stats(entries)
        print("%s: %d/%d entries translated" % (args.list, st["all"]["ok"], st["all"]["total"]), file=sys.stderr)
        return 0
    if args.cmd == "config":
        text, inline, shared = gen_config(pob, shape, parse_range(args.range))
        _write(args.out, text)
        st = configopts.stats(inline, shared)
        print("ConfigOptions: %d/%d apply closures translated" % (st["inline"]["ok"], st["inline"]["total"]), file=sys.stderr)
        return 0
    if args.cmd == "coverage":
        rep = coverage(pob, shape)
        print_coverage(rep)
        if args.json:
            _write(args.json, json.dumps(rep, indent=1, sort_keys=True) + "\n")
        return 0
    return 2
