#!/usr/bin/env python3
"""Derive the DF-B unit corpus (calc doc §9.3 `unit/<id>`) from an all57 build.

Each variant is `.parity/builds/allflame_exp_031.xml` (Shield Crush: a shield in Weapon 2 and a
body armour) with config inputs and a custom modifier block that reach branches of
CalcDefence.lua 659-1210 the all57 corpus leaves cold (ward, Iron Reflexes, the
"<defence> increased by <resistance>" flags, armour-to-ES-recharge, mana to armour, ES recovery
caps, lucky/unlucky/unexciting rolls, the EHP-unlucky config, Acrobatics-style suppression to
dodge, blocked-hit damage).

    python3 -I crates/pob-oracle/corpus/DF-B/derive.py \
        /home/ferm/Work/projects/PathOfBuildingMK2/.parity/builds/allflame_exp_031.xml \
        crates/pob-oracle/corpus/DF-B
"""
import sys

VARIANTS = {
    "dfb_ward_ironreflexes": (
        [],
        [
            "Iron Reflexes",
            "Increases and Reductions to Maximum Energy Shield instead apply to Ward",
            "Gain Ward instead of 50% of Armour and Evasion Rating from Equipped Body Armour",
            "+200 to Ward",
            "+150 to maximum Energy Shield",
            "+300 to Evasion Rating",
            "Chance to Evade Hits is based off of 50% of your Ward instead of your Evasion Rating",
            "Armour is increased by Uncapped Fire Resistance",
            "Evasion Rating is increased by Overcapped Cold Resistance",
            "Maximum Energy Shield is increased by Chaos Resistance",
            "Increases and Reductions to Armour also apply to Energy Shield Recharge Rate at 40% of their value",
            "20% of Maximum Mana is Converted to twice that much Armour",
            "Cannot Recover Energy Shield to above Armour",
            "Your chance to Block is Lucky",
            "You take 30% of Damage from Blocked Hits",
            "Modifiers to Chance to Suppress Spell Damage instead apply to Chance to Dodge Spell Hits at 50% of their value",
            "+24% chance to Suppress Spell Damage",
        ],
    ),
    "dfb_unexciting_unlucky": (
        [
            '<Input name="EHPUnluckyWorstOf" number="4"/>',
            '<Input boolean="true" name="conditionLowEnergyShield"/>',
            '<Input name="enemyLevel" number="75"/>',
        ],
        [
            "Your Lucky or Unlucky effects are instead Unexciting",
            "Chance to Block Spell Damage is equal to Chance to Block Attack Damage",
            "You are at Maximum Chance to Block Attack Damage if you have not Blocked Recently",
            "+35% chance to Suppress Spell Damage",
            "Your chance to Suppress Spell Damage is Lucky",
            "Cannot Recover Energy Shield to above Armour",
            "+500 to maximum Energy Shield",
        ],
    ),
}

MARKER = '\t\t<ConfigSet id="1">\n'


def main() -> None:
    base_path, out_dir = sys.argv[1], sys.argv[2]
    with open(base_path, encoding="utf-8") as f:
        base = f.read()
    if MARKER not in base:
        raise SystemExit("no <ConfigSet id=\"1\"> in the base build")
    for name, (inputs, mods) in VARIANTS.items():
        extra = "".join(f"\t\t\t{i}\n" for i in inputs)
        block = '\t\t\t<CustomModifierBlock title="DF-B" enabled="true">' + "\n".join(mods) + "</CustomModifierBlock>\n"
        text = base.replace(MARKER, MARKER + extra + block, 1)
        with open(f"{out_dir}/{name}.xml", "w", encoding="utf-8") as f:
            f.write(text)
        print(name)


if __name__ == "__main__":
    main()
