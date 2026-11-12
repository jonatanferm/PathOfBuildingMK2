#!/usr/bin/env python3
"""Summarise scripts/port/fresh_corpus_run.sh results (INT-1-FRESH).

Usage: python3 -I scripts/port/fresh_corpus_summary.py [--fresh DIR] [--probe DIR] [--families]

Reads DIR/results/*.log (the CA test's stderr per chunk) and *.tsv (CA_L6_DIFFS) and prints
  * totals: the chunks' `CA L6:` / `CA D13:` lines summed;
  * one row per build: headline / keys / CALCS lists / used-tracking / misc mismatches, Rust
    errors, D13;
  * with --families: mismatching main-output keys grouped by key family (the key with the
    `MainHand.` / `OffHand.` / `Minion.` prefix and list indices folded) and builds.
PoB failures come from the CA test's cache (`--probe`, default <worktree>/target/tmp/ca-probe):
`<stem>/error.txt`.
"""
import argparse
import collections
import os
import re


def find_fresh():
    d = os.path.dirname(os.path.abspath(__file__))
    while d != "/":
        if os.path.isdir(os.path.join(d, ".parity", "fresh")):
            return os.path.join(d, ".parity", "fresh")
        d = os.path.dirname(d)
    return None


def main():
    root = os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
    ap = argparse.ArgumentParser()
    ap.add_argument("--fresh", default=find_fresh())
    ap.add_argument("--probe", default=os.path.join(root, "target", "tmp", "ca-probe"))
    ap.add_argument("--families", action="store_true")
    a = ap.parse_args()
    res = os.path.join(a.fresh, "results")
    tot = collections.Counter()
    builds = collections.OrderedDict()
    rx_sum = re.compile(
        r"CA L6: (\d+) builds \((\d+) PoB runs failed\), (\d+) Rust runs raised; headline (\d+)/(\d+) exact .*?"
        r"(\d+) builds with every headline key exact; all keys (\d+)/(\d+) exact .*?; CALCS lists (\d+)/(\d+); "
        r"used-tracking key sets (\d+)/(\d+); misc calculator (\d+)/(\d+)")
    rx_d13 = re.compile(r"CA D13: (\d+) builds, (\d+) ran, (\d+) fixed points, (\d+) differ, (\d+) Lua errors")
    names = ["builds", "pob_failed", "rust_raised", "headline_exact", "headline", "builds_headline_exact",
             "keys_exact", "keys", "calcs_exact", "calcs", "used_equal", "used", "misc_exact", "misc"]
    for f in sorted(os.listdir(res)):
        if not f.endswith(".log") or ".." not in f:
            continue
        text = open(os.path.join(res, f), errors="replace").read()
        for m in rx_sum.finditer(text):
            tot.update(dict(zip(names, map(int, m.groups()))))
        for m in rx_d13.finditer(text):
            tot.update(dict(zip(["d13_builds", "d13_ran", "d13_fixed", "d13_differ", "d13_errors"], map(int, m.groups()))))
        for line in text.splitlines():
            m = re.match(r"L6 (\S+): keys (\d+)/(\d+) exact \((\d+) Rust-only\); headline (.*)", line)
            if m:
                b = builds.setdefault(m.group(1), collections.defaultdict(list))
                b["keys"] = (int(m.group(2)), int(m.group(3)), int(m.group(4)))
                b["headline"] = m.group(5)
                continue
            m = re.match(r"L6 (\S+): Rust (.*)", line)
            if m:
                builds.setdefault(m.group(1), collections.defaultdict(list))["errors"].append(m.group(2))
                continue
            m = re.match(r"D13 (\S+): (.*)", line)
            if m:
                builds.setdefault(m.group(1), collections.defaultdict(list))["d13"] = m.group(2)
                continue
            m = re.match(r"    (.*)", line)
    diffs = collections.defaultdict(list)
    for f in sorted(os.listdir(res)):
        if f.endswith(".tsv"):
            for line in open(os.path.join(res, f), errors="replace"):
                p = line.rstrip("\n").split("\t")
                if len(p) == 5:
                    diffs[p[0]].append(p[1:])
    pob_failed = {}
    for s in sorted(os.listdir(a.fresh)):
        if s.endswith(".xml"):
            e = os.path.join(a.probe, s[:-4], "error.txt")
            if os.path.isfile(e):
                pob_failed[s[:-4]] = open(e, errors="replace").read().strip().splitlines()[0][:200]
    print("## totals")
    for k in names + ["d13_builds", "d13_ran", "d13_fixed", "d13_differ", "d13_errors"]:
        print(f"{k}: {tot[k]}")
    print(f"pob_failed (error.txt): {len(pob_failed)}")
    for s, e in pob_failed.items():
        print(f"  {s}: {e}")
    print()
    print("## per build")
    print()
    print("| stem | headline exact | keys exact / compared | CALCS lists not exact | used-tracking sets differing "
          "| misc values not exact | Rust errors | D13 |")
    print("|---|---|---|---|---|---|---|---|")
    for s, b in builds.items():
        d = diffs.get(s, [])
        c = collections.Counter(x[0].split(".")[0] for x in d)
        k = b.get("keys")
        hl = b.get("headline", "-")
        hl = "13/13" if hl == "exact" else f"{13 - hl.count(' vs PoB ')}/13"
        keys = f"{k[0]}/{k[1] + k[2]}" if k else "-"
        if k and k[2]:
            keys += f" ({k[2]} Rust-only)"
        errs = "; ".join(e[:120] for e in b.get("errors", [])) or "0"
        d13 = b.get("d13", "-")
        d13 = "fixed point" if d13.startswith("ok") else d13[:100]
        print(f"| {s} | {hl} | {keys} | {c['calcs']} | {c['used']} | {c['misc']} | {errs} | {d13} |")
    if a.families:
        print()
        print("## main-output key families (keys, builds)")
        fam = collections.defaultdict(lambda: [0, set()])
        for s, d in diffs.items():
            for blk, key, ours, pob in d:
                if blk != "main":
                    continue
                k = re.sub(r"^(MainHand|OffHand|Minion)\.", "", key)
                k = re.sub(r"\.\d+(?=\.|$)", ".#", k)
                fam[k][0] += 1
                fam[k][1].add(s)
        for k, (n, bs) in sorted(fam.items(), key=lambda kv: (-len(kv[1][1]), -kv[1][0])):
            print(f"{n:5d} {len(bs):3d}  {k}  [{', '.join(sorted(bs)[:6])}]")


if __name__ == "__main__":
    main()
