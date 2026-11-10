#!/usr/bin/env python3
"""Derive the DF-D unit corpus (calc doc §9.3 `unit/<id>`).

CalcDefence.lua 1681-2152 branches on config inputs the all57 corpus never
sets: `enemyDamageType` (every build uses the default "Average"),
`EHPUnluckyWorstOf`, explicit enemy damage / pen / overwhelm / crit inputs and
boss skill presets (enemy skill conversion, incl. the >100% cap at 1773).
This script copies a few all57 builds and sets those inputs in the active
config set. Run from anywhere:

    python3 crates/pob-oracle/corpus/DF-D/derive.py [--parity .parity]

and refill the dumps with `scripts/port/oracle.sh run --build <xml> --stage
buildDefenceEstimations:1681,buildDefenceEstimations:2153` per output file.
"""
import argparse
import os
import re
import sys

HERE = os.path.dirname(os.path.abspath(__file__))

# base build stem -> why
BASES = {
    "allflame_exp_000": "armour, 100% spell suppression",
    "allflame_exp_020": "ward, armour, 97.75% suppression",
    "allflame_exp_005": "very high armour",
}

CUSTOM = [
    "20% of Armour also applies to Chaos Damage taken from Hits",
    "30% of Armour applies to Fire, Cold and Lightning Damage taken from Hits",
    "Physical Damage Reduction from Hits is based off of 50% of your Ward instead of your Armour",
    "Lightning Resistance does not affect Lightning Damage taken",
    "Fire Resistance cannot be Penetrated",
    "20% of Fire and Lightning Damage from Hits taken as Cold Damage",
    "10% of Cold Damage taken as Lightning",
    "-40 Physical Damage taken from Attack Hits",
    "+25 Fire Damage taken from Hits",
    "10% increased Damage taken",
    "5% reduced Cold Damage taken over time",
]

# variant -> [(name, kind, value)] (values are written into the XML as given)
VARIANTS = {
    "dot": [("enemyDamageType", "string", "DamageOverTime")],
    "untyped_w2": [("enemyDamageType", "string", "Untyped"), ("EHPUnluckyWorstOf", "number", "2")],
    "melee_w4": [
        ("enemyDamageType", "string", "Melee"),
        ("EHPUnluckyWorstOf", "number", "4"),
        ("enemyPhysicalDamage", "number", "8000"),
        ("enemyFireDamage", "number", "3000"),
        ("enemyFirePen", "number", "15"),
        ("enemyPhysicalOverwhelm", "number", "10"),
        ("enemyCritChance", "number", "40"),
        ("enemyCritDamage", "number", "80"),
    ],
    "projectile": [("enemyDamageType", "string", "Projectile")],
    "spell": [
        ("enemyDamageType", "string", "Spell"),
        ("enemyColdDamage", "number", "2500"),
        ("enemyLightningPen", "number", "20"),
    ],
    "spellproj": [("enemyDamageType", "string", "SpellProjectile"), ("enemyChaosDamage", "number", "1500")],
    # player mods for the rarer branches: armour applying partially / to other
    # types, ward-based physical reduction, SelfIgnore / EnemyCannotPen flags,
    # "taken as" shifts, flat and increased damage taken
    "custom": [("customMods", "string", "&#10;".join(CUSTOM))],
    "custom_spell": [("enemyDamageType", "string", "Spell"), ("customMods", "string", "&#10;".join(CUSTOM))],
    "custom_melee": [("enemyDamageType", "string", "Melee"), ("customMods", "string", "&#10;".join(CUSTOM))],
    "sirus": [("enemyIsBoss", "string", "Pinnacle"), ("presetBossSkills", "string", "Sirus Meteor")],
    "maven": [("enemyIsBoss", "string", "Pinnacle"), ("presetBossSkills", "string", "Maven Memory Game")],
}


def set_inputs(xml: str, inputs) -> str:
    m = re.search(r'<Config[^>]*activeConfigSet="([^"]+)"', xml)
    active = m.group(1) if m else "1"
    sm = re.search(r'(<ConfigSet[^>]*id="%s"[^>]*>)(.*?)(</ConfigSet>)' % re.escape(active), xml, re.S)
    if not sm:
        sys.exit("no active ConfigSet")
    body = sm.group(2)
    for name, kind, value in inputs:
        body = re.sub(r'\s*<Input[^>]*name="%s"[^>]*/>' % re.escape(name), "", body)
        body += '\n\t\t\t<Input name="%s" %s="%s"/>' % (name, kind, value)
    body += "\n\t\t"
    return xml[: sm.start(2)] + body + xml[sm.end(2):]


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--parity", default=None)
    a = ap.parse_args()
    parity = a.parity
    if parity is None:
        d = HERE
        while d != "/":
            if os.path.isdir(os.path.join(d, ".parity", "builds")):
                parity = os.path.join(d, ".parity")
                break
            d = os.path.dirname(d)
    if parity is None and os.path.isdir("/home/ferm/Work/projects/PathOfBuildingMK2/.parity/builds"):
        parity = "/home/ferm/Work/projects/PathOfBuildingMK2/.parity"
    if parity is None:
        sys.exit("no .parity/builds")
    for stem in BASES:
        with open(os.path.join(parity, "builds", stem + ".xml"), encoding="utf-8") as f:
            xml = f.read()
        for var, inputs in VARIANTS.items():
            out = os.path.join(HERE, "dfd_%s_%s.xml" % (stem.replace("allflame_exp_", "af"), var))
            with open(out, "w", encoding="utf-8") as f:
                f.write(set_inputs(xml, inputs))
            print(out)


if __name__ == "__main__":
    main()
