#!/usr/bin/env python3
"""DF-G unit corpus: config variants of slice5_allflame_exp_024 (Winter Orb) that reach the
branches of CalcDefence.lua 3165-3873 the corpus builds rarely take.

* dfg_dot_config: enemyDamageType = DamageOverTime with two configured enemy damages (the
  3547-3606 branch: `Config` and `Default` sources of `TotalDegen`, then the Comprehensive* net
  regen block 3680-3784).
* dfg_enemy_crit_ailments: enemy crit chance 30 (> enemyCritThreshold 10.1), so the enemy
  ignite chance is positive and the ailment branch 3607-3676 runs.

Run from this directory: python3 derive.py (rewrites the .xml files).
"""
import os

HERE = os.path.dirname(os.path.abspath(__file__))
SRC = os.path.join(HERE, "..", "U0", "slice5", "slice5_allflame_exp_024.xml")
ANCHOR = '\t\t\t<Input name="BrandsAttachedToEnemy" number="1"/>\n'
VARIANTS = {
    "dfg_dot_config": [
        '<Input name="enemyDamageType" string="DamageOverTime"/>',
        '<Input name="enemyFireDamage" number="1500"/>',
        '<Input name="enemyChaosDamage" number="400"/>',
    ],
    "dfg_enemy_crit_ailments": ['<Input name="enemyCritChance" number="30"/>'],
}


def main():
    src = open(SRC, encoding="utf-8").read()
    assert ANCHOR in src, "anchor line not found"
    for name, lines in VARIANTS.items():
        add = "".join("\t\t\t" + l + "\n" for l in lines)
        with open(os.path.join(HERE, name + ".xml"), "w", encoding="utf-8") as f:
            f.write(src.replace(ANCHOR, add + ANCHOR, 1))
        print(name)


if __name__ == "__main__":
    main()
