#!/usr/bin/env python3
"""Derive the DF-F unit corpus (calc doc §9.3 `unit/<id>`): config variants of
two all57 builds that exercise recoup, life-loss prevention and net recoup
(allflame_exp_044, allflame_exp_016), so the EHP segment
(CalcDefence.lua 2612-3164) also runs its PvP branch, every enemy damage
category (incl. DamageOverTime), the unlucky avoidance config and gain on
block.

Usage: derive.py [PARITY_DIR]   (default: the enclosing checkout's .parity)
Writes <stem>__<variant>.xml next to this script.
"""
import os
import re
import sys

BASES = ["allflame_exp_044", "allflame_exp_016"]
# variant -> (inputs to set, input names to remove)
VARIANTS = {
    "pvp": ({"PvpScaling": ("boolean", "true"), "enemyMultiplierPvpDamage": ("number", "150")}, []),
    "spell_unlucky4": ({"enemyDamageType": ("string", "Spell"), "EHPUnluckyWorstOf": ("number", "4")}, []),
    "projectile_unlucky2": ({"enemyDamageType": ("string", "Projectile"), "EHPUnluckyWorstOf": ("number", "2")}, []),
    "melee_gainonblock": ({"enemyDamageType": ("string", "Melee")}, ["DisableEHPGainOnBlock"]),
    "spellprojectile": ({"enemyDamageType": ("string", "SpellProjectile")}, ["DisableEHPGainOnBlock"]),
    "untyped": ({"enemyDamageType": ("string", "Untyped")}, []),
    "dot": ({"enemyDamageType": ("string", "DamageOverTime")}, []),
}


def find_parity():
    d = os.path.dirname(os.path.abspath(__file__))
    while d != "/":
        if os.path.isdir(os.path.join(d, ".parity", "builds")):
            return os.path.join(d, ".parity")
        d = os.path.dirname(d)
    sys.exit("no .parity/builds above this script; pass PARITY_DIR")


def active_set_span(xml):
    m = re.search(r'<Config activeConfigSet="(\d+)">', xml)
    if not m:
        sys.exit("no <Config activeConfigSet=...>")
    sid = m.group(1)
    start = xml.index(f'<ConfigSet id="{sid}"', m.end())
    open_end = xml.index(">", start) + 1
    close = xml.index("</ConfigSet>", open_end)
    return open_end, close


def apply(xml, sets, removes):
    a, b = active_set_span(xml)
    body = xml[a:b]
    for name in list(sets) + removes:
        body = re.sub(r'\n?[ \t]*<Input[^>]*\bname="' + re.escape(name) + r'"[^>]*/>', "", body)
    lines = "".join(f'\n\t\t\t<Input name="{k}" {t}="{v}"/>' for k, (t, v) in sets.items())
    return xml[:a] + lines + body + xml[b:]


def main():
    parity = sys.argv[1] if len(sys.argv) > 1 else find_parity()
    out = os.path.dirname(os.path.abspath(__file__))
    for stem in BASES:
        with open(os.path.join(parity, "builds", stem + ".xml"), encoding="utf-8") as f:
            xml = f.read()
        for var, (sets, removes) in VARIANTS.items():
            p = os.path.join(out, f"{stem}__{var}.xml")
            with open(p, "w", encoding="utf-8") as f:
                f.write(apply(xml, sets, removes))
            print(p)


if __name__ == "__main__":
    main()
