#!/usr/bin/env python3
"""Derive the PF-A unit corpus from all57 (.parity/builds).

pfa_cached_outputs.xml: allflame_exp_024 (Winter Orb; its non-main spells are enabled) plus a
socket group with Blight and Penance Brand of Dissipation at skillPart 2, so perform's
"maximum sustainable stages" loop calls getCachedOutputValue (CalcPerform.lua:2278-2290).

Usage: derive.py [PARITY_DIR]   (default: the .parity of the enclosing checkout)
"""
import os
import sys

here = os.path.dirname(os.path.abspath(__file__))


def find_parity():
    d = here
    while d != "/":
        if os.path.isdir(os.path.join(d, ".parity", "builds")):
            return os.path.join(d, ".parity")
        d = os.path.dirname(d)
    sys.exit("no .parity/builds found")


GROUP = """\t\t\t<Skill enabled="true" includeInFullDPS="nil" label="PF-A getCachedOutputValue" mainActiveSkill="nil" mainActiveSkillCalcs="nil">
\t\t\t\t<Gem count="nil" enableGlobal1="true" enableGlobal2="true" enabled="true" gemId="Metadata/Items/Gems/SkillGemBlight" level="20" nameSpec="Blight" quality="0" skillId="Blight" skillPart="2" variantId="Blight"/>
\t\t\t\t<Gem count="nil" enableGlobal1="true" enableGlobal2="true" enabled="true" gemId="Metadata/Items/Gems/SkillGemPenanceBrandAltX" level="20" nameSpec="Penance Brand of Dissipation" quality="0" skillId="PenanceBrandAltX" skillPart="2" variantId="PenanceBrandAltX"/>
\t\t\t</Skill>
\t\t</SkillSet>"""


def main():
    parity = sys.argv[1] if len(sys.argv) > 1 else find_parity()
    src = open(os.path.join(parity, "builds", "allflame_exp_024.xml"), encoding="utf-8").read()
    marker = "\t\t</SkillSet>"
    if src.count(marker) != 1:
        sys.exit("allflame_exp_024.xml: expected exactly one </SkillSet>")
    out = src.replace(marker, GROUP, 1)
    with open(os.path.join(here, "pfa_cached_outputs.xml"), "w", encoding="utf-8") as f:
        f.write(out)


if __name__ == "__main__":
    main()
