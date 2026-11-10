#!/usr/bin/env python3
"""SK-A unit corpus (calc doc §9.3 `unit/<id>`): one synthetic build per act_int skill hook skill
and Calcs-tab skill part, so that every Data/Skills/act_int.lua hook runs under the oracle with
and without a breakdown.

Every build is corpus/U0/slice5/slice5_allflame_exp_024.xml (Witch, Wand + Shield, life and ES,
`BrandsAttachedToEnemy` = 1) with its <Skills> replaced:

* group 1 (the main socket group): the hook skill at level 20 with `skillPart = skillPartCalcs =
  <part>`, so the CALCS env (the Calcs tab, the only env with a breakdown) and the MAIN env both
  run the hook on that part;
* only in a skill's first build: one `includeInFullDPS` group per skill part (and, for the hooks
  that branch on `skillFlags.totem`, the same with Spell Totem), which `calcs.calcFullDPS` runs
  through `calcs.perform` with every group as the main skill (CALCULATOR mode, no breakdown);
* Bane / Bane of Condemnation are linked to two curses (Despair, Temporal Chains), so the
  preSkillTypeFunc counts skills with `AppliedByBane`.

Skills whose breakdown text depends on the part get one build per part; the others one build
(the part that exercises the hook's skillPart branch when it has one).

Usage: gen_builds.py [--pob-dump DIR]    (writes corpus/SK-A/builds/*.xml and builds.txt)
"""
import json
import os
import re
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
TEMPLATE = os.path.join(HERE, "..", "U0", "slice5", "slice5_allflame_exp_024.xml")
OUT = os.path.join(HERE, "builds")


def find_dump():
    if "--pob-dump" in sys.argv:
        return sys.argv[sys.argv.index("--pob-dump") + 1]
    d = HERE
    while d != "/":
        p = os.path.join(d, "data", "dump", "closures.json")
        if os.path.isfile(p):
            return os.path.dirname(p)
        d = os.path.dirname(d)
    raise SystemExit("no data/dump (pass --pob-dump DIR)")


# Hooks whose breakdown output depends on the skill part: one build per part.
PER_PART = {
    "BallLightning", "BallLightningAltX", "ForbiddenRite", "ForbiddenRiteAltX",
    "KineticFusillade", "KineticFusilladeAltX",
    "LightningSpireTrap", "LightningSpireTrapAltX", "LightningSpireTrapAltY",
}
# Hooks that branch on skillFlags.totem.
TOTEM = {
    "Bodyswap", "BodyswapAltX", "DarkPact", "DarkPactAltX", "ForbiddenRite", "ForbiddenRiteAltX",
    "RighteousFire", "VaalRighteousFire",
}
# Extra gems linked into the main group.
LINKS = {
    "Bane": ["Despair", "TemporalChains"],
    "BaneAltX": ["Despair", "TemporalChains"],
}
# Gem to use when several gems grant the skill (the plain gem, not the Vaal one).
GEM = {"VaalRighteousFire": "Metadata/Items/Gems/SkillGemVaalRighteousFire"}


def gem_xml(gems, gem_id, skill_part=None):
    g = gems[gem_id]
    part = ""
    if skill_part is not None:
        part = f' skillPart="{skill_part}" skillPartCalcs="{skill_part}"'
    return (
        f'\t\t\t\t<Gem count="nil" enableGlobal1="true" enableGlobal2="true" enabled="true" '
        f'gemId="{gem_id}" level="20" nameSpec="{g["name"]}" quality="0" '
        f'skillId="{g["grantedEffectId"]}"{part} variantId="{g["variantId"]}"/>'
    )


def group_xml(lines, full_dps, main_active=1):
    inc = "true" if full_dps else "nil"
    return (
        f'\t\t\t<Skill enabled="true" includeInFullDPS="{inc}" label="" '
        f'mainActiveSkill="{main_active}" mainActiveSkillCalcs="{main_active}">\n'
        + "\n".join(lines)
        + "\n\t\t\t</Skill>"
    )


def main():
    dump = find_dump()
    gems = json.load(open(os.path.join(dump, "gems.json")))
    skills = json.load(open(os.path.join(dump, "skills.json")))
    closures = json.load(open(os.path.join(dump, "closures.json")))["closures"]
    hook_skills = sorted({c["id"].split(".")[2] for c in closures if c["src"].startswith("Data/Skills/act_int.lua:")})
    by_effect = {}
    for gid, g in sorted(gems.items()):
        by_effect.setdefault(g["grantedEffectId"], gid)
    support_gem = {g["grantedEffectId"]: gid for gid, g in gems.items()}
    template = open(TEMPLATE).read()
    m = re.search(r"\t<Skills .*?</Skills>\n", template, re.S)
    assert m, "template has no <Skills>"
    keep = re.findall(r'\t\t\t<Skill enabled="false"[^>]*source="[^"]*">.*?</Skill>', m.group(0), re.S)
    os.makedirs(OUT, exist_ok=True)
    for f in os.listdir(OUT):
        if f.endswith(".xml"):
            os.remove(os.path.join(OUT, f))
    names = []
    for sid in hook_skills:
        gem_id = GEM.get(sid) or by_effect.get(sid)
        assert gem_id, f"no gem grants {sid}"
        effects = [e if isinstance(e, str) else (e.get("$ref") or e.get("id") or e.get("$data") or "") for e in gems[gem_id].get("grantedEffectList") or []]
        main_active = next((i + 1 for i, e in enumerate(effects) if e.endswith("." + sid)), 1)
        nparts = len(skills[sid].get("parts") or [])
        parts = list(range(1, nparts + 1)) if sid in PER_PART else [min(2, nparts) if nparts else None]
        links = [gem_xml(gems, support_gem[l]) for l in LINKS.get(sid, [])]
        for i, part in enumerate(parts):
            groups = [group_xml([gem_xml(gems, gem_id, part)] + links, False, main_active)]
            if i == 0:
                for p in (range(1, nparts + 1) if nparts else [None]):
                    groups.append(group_xml([gem_xml(gems, gem_id, p)] + links, True, main_active))
                    if sid in TOTEM:
                        groups.append(group_xml([gem_xml(gems, gem_id, p), gem_xml(gems, support_gem["SupportSpellTotem"])], True, main_active))
            skills_xml = (
                '\t<Skills activeSkillSet="1" defaultGemLevel="normalMaximum" defaultGemQuality="nil" '
                'showLegacyGems="false" showSupportGemTypes="ALL" sortGemsByDPS="true" sortGemsByDPSField="CombinedDPS">\n'
                '\t\t<SkillSet id="1">\n' + "\n".join(groups + keep) + "\n\t\t</SkillSet>\n\t</Skills>\n"
            )
            xml = template[: m.start()] + skills_xml + template[m.end():]
            xml = xml.replace(
                "<PathOfBuilding>",
                f"<PathOfBuilding>\n\t<!-- SK-A synthetic (corpus/SK-A/gen_builds.py): {sid}, Calcs part {part} -->",
                1,
            )
            name = f"ska_{sid}" + (f"_p{part}" if part is not None and sid in PER_PART else "")
            open(os.path.join(OUT, name + ".xml"), "w").write(xml)
            names.append(name)
    open(os.path.join(HERE, "builds.txt"), "w").write(
        "# SK-A corpus: generated by gen_builds.py (one line per build in builds/)\n" + "\n".join(names) + "\n"
    )
    print(len(names), "builds")


if __name__ == "__main__":
    main()
