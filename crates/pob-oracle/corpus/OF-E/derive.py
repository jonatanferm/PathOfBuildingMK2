#!/usr/bin/env python3
"""Derive the OF-E unit corpus (calc doc §9.3 `unit/<id>`) from the slice5 builds.

Each variant is a slice5 build (corpus/U0/slice5) whose Config tab custom modifier
block holds the lines below, chosen to reach the branches of CalcOffence.lua
2607-3345 the corpus builds do not: lucky / extreme luck / unexciting crit rolls,
bifurcation, guaranteed crits every 3rd use, the crit chance cap and overcap
breakdown, NeverCrit, NoCritMultiplier, double / triple damage, culling, the
crit-multiplier-to-DoT conversion and the Cryogenesis added damage redirection.

Usage: python3 derive.py            (rewrites the variant XMLs next to this file)
       python3 derive.py --check    (fails when a variant XML is stale)
"""
import pathlib
import sys

HERE = pathlib.Path(__file__).resolve().parent
SLICE5 = HERE.parent / "U0" / "slice5"

VARIANTS = {
    # Spark (spell, projectile; no crit multiplier): lucky with extreme luck, double/triple
    # damage, culling on crit, crit multiplier applying to DoT
    "ofe_spell_lucky_extreme": ("slice5_allflame_exp_007", [
        "your critical strike chance is lucky",
        "your lucky or unlucky effects use the best or worst from three rolls instead of two",
        "your critical strikes have a 30% chance to deal double damage",
        "20% chance to deal triple damage",
        "critical strikes have culling strike",
        "modifiers to critical strike multiplier also apply to damage over time multiplier for ailments from critical strikes at 50% of their value",
    ]),
    # Winter Orb (spell): bifurcation (chance and multiplier), lucky
    "ofe_spell_bifurcate": ("slice5_allflame_exp_024", [
        "spell critical strike chance bifurcates",
        "your critical strike chance is lucky",
        "150% increased critical strike chance",
    ]),
    # Ethereal Knives (spell): unexciting rolls, crit chance cap and overcap breakdown, culling
    "ofe_spell_unexciting_cap": ("slice5_allflame_exp_030", [
        "your critical strike chance is lucky",
        "your lucky or unlucky effects are instead unexciting",
        "+150% to critical strike chance",
        "maximum critical strike chance is 40%",
        "kill enemies that have 15% or lower life when hit by your skills",
    ]),
    # Flicker Strike (travel attack): crit every 3rd use, lucky, Cryogenesis redirection
    "ofe_travel_every3_cold": ("slice5_allflame_exp_048", [
        "your travel skills critically strike once every 3 uses",
        "your critical strike chance is lucky",
        "you gain added cold damage instead of added damage of other types if dexterity exceeds both other attributes",
        "adds 10 to 20 fire damage to attacks",
        "adds 5 to 30 lightning damage to attacks",
    ]),
    # Shield Crush (attack): NeverCrit
    "ofe_attack_nevercrit": ("slice5_allflame_exp_031", [
        "never deal critical strikes",
    ]),
    # Shield Crush (attack): NoCritMultiplier, double damage on crit, culling strike
    "ofe_attack_nocritmulti": ("slice5_allflame_exp_031", [
        "critical strikes do not deal extra damage",
        "your critical strikes have a 30% chance to deal double damage",
        "culling strike",
    ]),
}

OPEN = '<CustomModifierBlock enabled="true" title="Default">'


def derive(src: str, lines: list) -> str:
    text = (SLICE5 / f"{src}.xml").read_text()
    a = text.index(OPEN) + len(OPEN)
    b = text.index("</CustomModifierBlock>", a)
    return text[:a] + "\n" + "\n".join(lines) + "\n\t\t\t" + text[b:]


def main() -> int:
    check = "--check" in sys.argv[1:]
    stale = []
    for name, (src, lines) in VARIANTS.items():
        out = HERE / f"{name}.xml"
        want = derive(src, lines)
        if check:
            if not out.is_file() or out.read_text() != want:
                stale.append(name)
        else:
            out.write_text(want)
    if stale:
        print("stale:", ", ".join(stale))
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
