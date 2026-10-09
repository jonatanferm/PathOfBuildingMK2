#!/usr/bin/env python3
"""Compare calc output trees key by key: a reference (normally PoB, from
pob_oracle) against a candidate (normally the Rust calc), and print a
parity scoreboard.

Usage:
  scripts/parity/diff.py REF CAND [--key KEY] [--top N] [--builds] [--only REGEX]
                                   [--abs 0.5] [--rel 0.01] [--fail]

REF / CAND are either single output JSON files or directories. In a
directory, build <stem> is <dir>/<stem>/outputs.json (the oracle layout,
scripts/parity/oracle_dump.sh) or <dir>/<stem>.json; builds are paired by
stem.

A file may be a pob_oracle outputs dump ({"player": {...}, "minion": {...}})
or any JSON object of outputs. Trees are flattened to dotted paths
(`MainHand.Speed`, `SkillDPS.1.dps`; minion keys under `Minion.`);
`{"$num": "inf"|"-inf"|"nan"}` decodes to the number, other `{"$...": x}`
reference markers compare as their text. See crates/pob-oracle/README.md.

Numbers match when |a-b| < ABS (0.5) or |a-b| <= REL (1%) of the larger
magnitude; booleans and strings must be identical; a type change is a
mismatch. A key missing from CAND counts as "missing" when its REF value is
non-zero / true / a non-empty string.
"""
import collections
import json
import math
import os
import re
import sys

HEADLINE = [
    "Life", "Mana", "EnergyShield", "Ward", "Str", "Dex", "Int",
    "LifeUnreserved", "ManaUnreserved", "Armour", "Evasion",
    "FireResist", "ColdResist", "LightningResist", "ChaosResist",
    "BlockChance", "SpellBlockChance", "SpellSuppressionChance",
    "LifeRegenRecovery", "ManaRegenRecovery", "EnergyShieldRecharge",
    "MovementSpeedMod", "TotalEHP", "PhysicalMaximumHitTaken",
    "AverageHit", "Speed", "CritChance", "TotalDPS", "FullDPS", "CombinedDPS",
]


def leaf(v):
    """Scalar for a JSON leaf (decoding {"$num": ...})."""
    if isinstance(v, dict) and len(v) == 1:
        (k, x), = v.items()
        if k == "$num":
            return {"inf": math.inf, "-inf": -math.inf, "nan": math.nan}.get(x, x)
        if k.startswith("$"):
            return json.dumps(v, sort_keys=True)
    return v


def flatten(tree, prefix="", out=None):
    out = {} if out is None else out
    v = leaf(tree)
    if isinstance(v, dict):
        for k, x in v.items():
            flatten(x, f"{prefix}.{k}" if prefix else k, out)
    elif isinstance(v, list):
        for i, x in enumerate(v, 1):
            flatten(x, f"{prefix}.{i}" if prefix else str(i), out)
    elif v is not None:
        out[prefix] = v
    return out


def load(path):
    with open(path) as f:
        doc = json.load(f)
    if isinstance(doc, dict) and "player" in doc and isinstance(doc["player"], dict):
        flat = flatten(doc["player"])
        if isinstance(doc.get("minion"), dict):
            flatten(doc["minion"], "Minion", flat)
        return flat
    return flatten(doc)


def collect(path):
    """stem -> file for a file or directory argument."""
    if os.path.isfile(path):
        stem = os.path.basename(os.path.dirname(path)) if os.path.basename(path) == "outputs.json" \
            else os.path.splitext(os.path.basename(path))[0]
        return {stem: path}
    found = {}
    for name in sorted(os.listdir(path)):
        p = os.path.join(path, name)
        if os.path.isdir(p) and os.path.isfile(os.path.join(p, "outputs.json")):
            found[name] = os.path.join(p, "outputs.json")
        elif name.endswith(".json") and os.path.isfile(p) and name != "outputs.json":
            found[name[:-5]] = p
    return found


def is_num(v):
    return isinstance(v, (int, float)) and not isinstance(v, bool)


def close(a, b, abs_tol, rel_tol):
    if is_num(a) and is_num(b):
        if math.isnan(a) or math.isnan(b):
            return math.isnan(a) and math.isnan(b)
        if math.isinf(a) or math.isinf(b):
            return a == b
        d = abs(a - b)
        return d < abs_tol or d <= rel_tol * max(abs(a), abs(b))
    return type(a) is type(b) and a == b


def significant(v):
    if is_num(v):
        return not (abs(v) < 0.5)
    return bool(v)


def fmt(v):
    if is_num(v):
        return f"{v:.6g}" if isinstance(v, float) else str(v)
    s = json.dumps(v)
    return s if len(s) <= 40 else s[:37] + "..."


def arg(name, default, conv=str):
    return conv(sys.argv[sys.argv.index(name) + 1]) if name in sys.argv else default


def main():
    flags_with_value = {"--key", "--top", "--only", "--abs", "--rel"}
    pos, skip = [], False
    for a in sys.argv[1:]:
        if skip:
            skip = False
        elif a in flags_with_value:
            skip = True
        elif not a.startswith("--"):
            pos.append(a)
    if len(pos) != 2:
        sys.exit(__doc__)
    key_filter = arg("--key", None)
    top = arg("--top", 40, int)
    only = re.compile(arg("--only", ".*"))
    abs_tol = arg("--abs", 0.5, float)
    rel_tol = arg("--rel", 0.01, float)

    ref_files, cand_files = collect(pos[0]), collect(pos[1])
    if len(ref_files) == 1 and len(cand_files) == 1:
        stems = [(next(iter(ref_files)), next(iter(cand_files)))]
    else:
        stems = [(s, s) for s in ref_files if s in cand_files]
    if not stems:
        sys.exit(f"no builds in common between {pos[0]} ({len(ref_files)}) and {pos[1]} ({len(cand_files)})")
    unpaired = sorted(set(ref_files) - set(cand_files)) if len(stems) > 1 or len(ref_files) > 1 else []

    shared = collections.Counter()
    div = collections.Counter()
    missing = collections.Counter()
    extra = collections.Counter()
    worst = {}
    per_build = []
    present = collections.Counter()
    for rs, cs in stems:
        ref = {k: v for k, v in load(ref_files[rs]).items() if only.search(k)}
        cand = {k: v for k, v in load(cand_files[cs]).items() if only.search(k)}
        if key_filter:
            r, c = ref.get(key_filter), cand.get(key_filter)
            mark = "" if (r is not None and c is not None and close(r, c, abs_tol, rel_tol)) else "  <--"
            print(f"{rs:<36} ref={fmt(r):>16} cand={fmt(c):>16}{mark}")
            continue
        b_shared = b_div = b_missing = 0
        for k, r in ref.items():
            present[k] += 1
            if k not in cand:
                if significant(r):
                    missing[k] += 1
                    b_missing += 1
                continue
            c = cand[k]
            shared[k] += 1
            b_shared += 1
            if not close(r, c, abs_tol, rel_tol):
                div[k] += 1
                b_div += 1
                if is_num(r) and is_num(c) and not (math.isinf(r) or math.isnan(r)):
                    sev = abs(c - r) / max(abs(r), 1) if not (math.isinf(c) or math.isnan(c)) else math.inf
                else:
                    sev = math.inf
                if k not in worst or sev > worst[k][0]:
                    worst[k] = (sev, rs, c, r)
        for k, c in cand.items():
            if k not in ref and significant(c):
                extra[k] += 1
        per_build.append((rs, b_shared - b_div, b_shared, b_missing))
    if key_filter:
        return

    n = len(stems)
    print(f"== {n} builds: {pos[0]} (ref) vs {pos[1]} (cand); tolerance |d|<{abs_tol} or <={rel_tol:.0%} ==")
    if unpaired:
        print(f"   ({len(unpaired)} ref builds without a candidate: {' '.join(unpaired[:10])}{' ...' if len(unpaired) > 10 else ''})")
    if "--builds" in sys.argv or n <= 10:
        print("\nPer build (matching / shared keys, ref keys missing in cand):")
        for stem, ok, sh, miss in per_build:
            print(f"  {stem:<36} {ok:>5}/{sh:<5} {100 * ok / max(sh, 1):6.2f}%   missing {miss}")

    print("\nHeadline stats (builds matching):")
    for k in HEADLINE:
        if not present[k]:
            continue
        m = shared[k] - div[k]
        w = worst.get(k)
        tail = f"   worst: {w[1]} cand={fmt(w[2])} ref={fmt(w[3])}" if w else ""
        print(f"  {k:<26} {m:>3}/{present[k]:<3}{tail}")
    total_shared, total_div = sum(shared.values()), sum(div.values())
    pct = 100 * (total_shared - total_div) / max(total_shared, 1)
    print(f"\nAll shared keys: {total_shared - total_div}/{total_shared} match ({pct:.2f}%)")
    print(f"Ref keys missing in cand (significant): {sum(missing.values())}; extra cand keys: {sum(extra.values())}")

    if div:
        print(f"\nMost often divergent keys (top {top}):")
        for k, c in div.most_common(top):
            w = worst[k]
            print(f"  {k:<44} {c:>3}/{shared[k]:<3}  worst: {w[1]} cand={fmt(w[2])} ref={fmt(w[3])}")
    if missing:
        print(f"\nRef keys the candidate never emits (significant in N builds), top {top}:")
        for k, c in missing.most_common(top):
            print(f"  {k:<44} {c:>3}")
    if extra:
        print(f"\nCandidate keys absent from ref, top {min(top, 20)}:")
        for k, c in extra.most_common(min(top, 20)):
            print(f"  {k:<44} {c:>3}")
    if "--fail" in sys.argv and (total_div or missing):
        sys.exit(1)


if __name__ == "__main__":
    main()
