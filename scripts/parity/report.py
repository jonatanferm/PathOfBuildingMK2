#!/usr/bin/env python3
"""Aggregate per-build {build, pob, ours} documents into a parity scoreboard.

Reads the --json-out output of pob_calc_diff (crates/pob-oracle; the legacy
pob_diff, deleted by INT-4, wrote the same shape): one <build>.json per build whose "pob" and "ours"
members map flattened output keys (`Life`, `MainHand.Speed`) to values.
Non-finite numbers may be written as {"$num": "inf"|"-inf"|"nan"}; other
non-numeric values (strings, booleans, markers) are not scored. Other
members of a document (pob_calc_diff's "mismatches", "summary") are ignored.

Usage: scripts/parity/report.py [.parity/results] [--key KEY] [--top N]
"""
import json, math, os, sys, glob, collections

HEADLINE = [
    "Life", "Mana", "EnergyShield", "Ward", "Str", "Dex", "Int",
    "LifeUnreserved", "ManaUnreserved", "Armour", "Evasion",
    "FireResist", "ColdResist", "LightningResist", "ChaosResist",
    "BlockChance", "SpellBlockChance", "SpellSuppressionChance",
    "LifeRegenRecovery", "ManaRegenRecovery", "EnergyShieldRecharge",
    "MovementSpeedMod", "TotalEHP", "PhysicalMaximumHitTaken",
    "AverageHit", "Speed", "CritChance", "TotalDPS", "FullDPS", "CombinedDPS",
]
ALIASES = {"Str": "Strength", "Dex": "Dexterity", "Int": "Intelligence"}

def close(a, b):
    if math.isnan(a) or math.isnan(b): return math.isnan(a) and math.isnan(b)
    if math.isinf(a) or math.isinf(b): return a == b
    d = abs(a - b)
    return d < 0.5 or d <= 0.01 * max(abs(a), abs(b))

def num(v):
    if isinstance(v, bool): return float(v)
    if isinstance(v, (int, float)): return float(v)
    if isinstance(v, dict) and len(v) == 1 and isinstance(v.get("$num"), str):
        return {"inf": math.inf, "-inf": -math.inf, "nan": math.nan}.get(v["$num"])
    return None

def main():
    args = [a for a in sys.argv[1:] if not a.startswith("--")]
    d = args[0] if args else ".parity/results"
    key_filter = sys.argv[sys.argv.index("--key") + 1] if "--key" in sys.argv else None
    top = int(sys.argv[sys.argv.index("--top") + 1]) if "--top" in sys.argv else 40
    docs = []
    for f in sorted(glob.glob(os.path.join(d, "*.json"))):
        with open(f) as fh:
            doc = json.load(fh)
        if isinstance(doc, dict) and isinstance(doc.get("pob"), dict) and isinstance(doc.get("ours"), dict):
            docs.append(doc)
    if not docs:
        sys.exit("no results")
    if key_filter:
        for doc in docs:
            p = num(doc["pob"].get(key_filter)); o = num(doc["ours"].get(ALIASES.get(key_filter, key_filter)))
            o = o if o is not None else num(doc["ours"].get(key_filter))
            flag = "" if (p is not None and o is not None and close(o, p)) else "  <--"
            print(f"{doc['build']:<28} pob={p!s:>14} ours={o!s:>14}{flag}")
        return
    div = collections.Counter(); shared = collections.Counter(); missing = collections.Counter()
    worst = {}
    for doc in docs:
        pob, ours = doc["pob"], doc["ours"]
        for k, pv in pob.items():
            p = num(pv)
            if p is None: continue
            o = num(ours.get(k))
            if o is None and k in ALIASES: o = num(ours.get(ALIASES[k]))
            if o is None:
                if abs(p) >= 0.5: missing[k] += 1
                continue
            shared[k] += 1
            if not close(o, p):
                div[k] += 1
                rel = abs(o - p) / max(abs(p), 1) if math.isfinite(o - p) else math.inf
                if rel > worst.get(k, (0,))[0]: worst[k] = (rel, doc["build"], o, p)
    n = len(docs)
    print(f"== {n} builds ==\n")
    print("Headline stats (builds matching within 1%):")
    ok_total = 0
    for k in HEADLINE:
        m = shared[k] - div[k]
        present = sum(1 for doc in docs if num(doc['pob'].get(k)) is not None)
        ok_total += m
        print(f"  {k:<26} {m:>3}/{present:<3}" + (f"   worst: {worst[k][1]} ours={worst[k][2]:.1f} pob={worst[k][3]:.1f}" if k in worst else ""))
    total_shared = sum(shared.values()); total_div = sum(div.values())
    print(f"\nAll shared keys: {total_shared - total_div}/{total_shared} match ({100*(total_shared-total_div)/max(total_shared,1):.1f}%)")
    print(f"\nMost often divergent keys (of {n} builds):")
    for k, c in div.most_common(top):
        print(f"  {k:<36} {c:>3}/{shared[k]:<3}  worst: {worst[k][1]} ours={worst[k][2]:.2f} pob={worst[k][3]:.2f}")
    print(f"\nPoB keys MK2 never emits (non-zero in N builds), top {top}:")
    for k, c in missing.most_common(top):
        print(f"  {k:<36} {c:>3}")

if __name__ == "__main__":
    main()
