#!/usr/bin/env python3
"""OF-B unit corpus (calc doc §9.3 `unit/<id>`): micro-builds for the branches of
CalcOffence.lua 1037-1691 that the all57 corpus never reaches as a main skill (mines, brands,
corpses, a trap with a cooldown).

Each build is a slice5 build (crates/pob-oracle/corpus/U0/slice5/) with the gems of its main
socket group replaced (or extended), so the rest of the character is a known-good PoB build.
Run from anywhere: `python3 crates/pob-oracle/corpus/OF-B/derive.py` rewrites the *.xml next to
this file. Probe them with
`scripts/port/oracle.sh run --build crates/pob-oracle/corpus/OF-B/<b>.xml --stage 'offence:1037#*,offence:1692#*'`.
"""
import os
import re

HERE = os.path.dirname(os.path.abspath(__file__))
SLICE5 = os.path.join(HERE, "..", "U0", "slice5")


def gem(gem_id, name, skill_id, level=20):
    return (
        '\t\t\t\t<Gem count="nil" enableGlobal1="true" enableGlobal2="true" enabled="true" '
        f'gemId="Metadata/Items/Gems/{gem_id}" level="{level}" nameSpec="{name}" quality="0" '
        f'skillId="{skill_id}" skillPart="1" skillPartCalcs="1" variantId="{skill_id}"/>\n'
    )


def derive(base, gems, keep=False):
    """`base`'s main socket group (mainActiveSkill="1" mainActiveSkillCalcs="1") with `gems`
    (appended to its gems when `keep`)."""
    text = open(os.path.join(SLICE5, base + ".xml")).read()
    m = re.search(
        r'(<Skill enabled="true"[^>]*mainActiveSkill="1" mainActiveSkillCalcs="1"[^>]*>\n)(.*?)(\t\t\t</Skill>)',
        text,
        re.S,
    )
    assert m, base
    body = (m.group(2) if keep else "") + "".join(gems)
    return text[: m.start(2)] + body + text[m.end(2) :]


BUILDS = {
    # mine laying speed/time, mine throw count, detonation radius (1345-1402); Icicle Mine is
    # also an Aura skill, so it reaches the mine aura radius (1403-1411) too
    "ofb_mine_icicle": ("slice5_allflame_exp_024", [gem("SkillGemIcicleMine", "Icicle Mine", "IcicleMine")], False),
    # a non-aura mine: a spell supported by Blastchain Mine (1345-1402 without 1403-1411)
    "ofb_mine_blastchain_fireball": (
        "slice5_allflame_exp_024",
        [
            gem("SkillGemFireball", "Fireball", "Fireball"),
            gem("SkillGemSupportBlastchainMine", "Blastchain Mine", "SupportBlastchainMine"),
        ],
        False,
    ),
    # brand attachment range / limit (1458-1466)
    "ofb_brand_armageddon": ("slice5_allflame_exp_024", [gem("SkillGemArmageddonBrand", "Armageddon Brand", "ArmageddonBrand")], False),
    # corpse level / life (1472-1488)
    "ofb_corpse_unearth": ("slice5_allflame_exp_024", [gem("SkillGemUnearth", "Unearth", "Unearth")], False),
    # trap throwing speed and a trap with a cooldown (1257-1318)
    "ofb_trap_fire": ("slice5_allflame_exp_024", [gem("SkillGemFireTrap", "Fire Trap", "FireTrap")], False),
}
# Momentum stacks (1038-1050) are reached by all57 (allflame_exp_021 adds the mod at 1043).

if __name__ == "__main__":
    for name, (base, gems, keep) in BUILDS.items():
        with open(os.path.join(HERE, name + ".xml"), "w") as f:
            f.write(derive(base, gems, keep))
        print(name)
