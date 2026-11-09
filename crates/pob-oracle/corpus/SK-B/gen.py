#!/usr/bin/env python3
"""Generate the SK-B oracle builds (crates/pob-oracle/tests/SK-B.rs).

One minimal build per (skill, skill part) whose main skill runs one of SK-B's
skill hooks (Data/Skills act_dex/act_str/other/minion/spectre/sup_str), with a
weapon the skill can use. The oracle probes `skill:<id>:<hook>:in/out` in
every env of these builds (MAIN without a breakdown, CALCS with one).

Usage: python3 gen.py [--dump DIR]   (DIR = data/dump; default: found upwards)
Writes builds/*.xml next to this script (the committed files are its output).
"""
import json
import os
import sys
from xml.sax.saxutils import escape

HERE = os.path.dirname(os.path.abspath(__file__))


def find_dump():
    if "--dump" in sys.argv:
        return sys.argv[sys.argv.index("--dump") + 1]
    d = HERE
    while d != "/":
        p = os.path.join(d, "data", "dump", "skills.json")
        if os.path.isfile(p):
            return os.path.dirname(p)
        d = os.path.dirname(d)
    sys.exit("data/dump not found")


# Skills with an SK-B hook (closures.json skill-hook ids outside act_int.lua).
HOOK_SKILLS = """
Barrage BarrageAltX BladeBlast BladeBlastAltX BladeFlurry BladeFlurryAltX BladeVortex
VaalBladeVortex BladefallAltZ BlastRain ChargedDash ChargedDashAltX Cremation CremationAltX
CremationAltY Cyclone CycloneAltX VaalCyclone ExplosiveArrow ExplosiveTrap ExplosiveTrapAltX
FlamethrowerTrap FlamethrowerTrapAltX LancingSteel LancingSteelAltX VaalLightningArrow StormRain
StormRainAltX StormRainAltY ScourgeArrow SeismicTrap SeismicTrapAltX ShrapnelBallista
ShrapnelBallistaAltX ToxicRain ToxicRainAltX ToxicRainAltY TornadoShot PoisonousConcoction
PoisonousConcoctionAltX Tornado TornadoAltY
Earthquake FrozenSweep FrozenSweepAltX HeraldOfAsh InfernalBlow InfernalBlowAltX MoltenShell
VaalMoltenShell MoltenStrike MoltenStrikeAltX Perforate PerforateAltX PerforateAltY RageVortex
StaticStrike
BloodSacramentUnique DeathWish HeraldOfTheBreach LightningBolt
""".split()

# weaponTypes key -> a normal base of that type, in preference order
WEAPONS = [
    ("Bow", "Short Bow"),
    ("Two Handed Sword", "Corroded Blade"),
    ("Two Handed Axe", "Stone Axe"),
    ("Two Handed Mace", "Driftwood Maul"),
    ("Staff", "Gnarled Branch"),
    ("One Handed Sword", "Rusted Sword"),
    ("Thrusting One Handed Sword", "Rusted Spike"),
    ("One Handed Axe", "Rusted Hatchet"),
    ("One Handed Mace", "Driftwood Club"),
    ("Sceptre", "Driftwood Sceptre"),
    ("Claw", "Nailed Fist"),
    ("Dagger", "Glass Shank"),
    ("Wand", "Driftwood Wand"),
]


def gem_attrs(gems, gid):
    """Gem attributes in the 3.23+ save format: gemId is the game id, variantId picks the
    transfigured variant (SkillsTab.lua:464-471)."""
    g = gems[gid]
    return {"gemId": g["gameId"], "variantId": g["variantId"], "skillId": g["grantedEffectId"]}


def item_xml(i, text):
    return f'\t\t<Item id="{i}">\n{escape(text)}\n\t\t</Item>\n'


def build_xml(groups, items, slots, main_group=1, spectres=()):
    """groups: list of (slot or None, [gem attr dicts], group attrs); items: [(id, text)];
    spectres: the build's spectre library (Build.lua:987-992)."""
    out = ['<?xml version="1.0" encoding="UTF-8"?>\n<PathOfBuilding>\n']
    head = (
        f'\t<Build level="90" targetVersion="3_0" className="Ranger" ascendClassName="None" '
        f'mainSocketGroup="{main_group}" viewMode="CALCS" bandit="None" pantheonMajorGod="None" pantheonMinorGod="None"'
    )
    if spectres:
        out.append(head + ">\n" + "".join(f'\t\t<Spectre id="{s}"/>\n' for s in spectres) + "\t</Build>\n")
    else:
        out.append(head + "/>\n")
    out.append('\t<Skills activeSkillSet="1" defaultGemLevel="normalMaximum" defaultGemQuality="0">\n\t\t<SkillSet id="1">\n')
    for slot, gems, gattrs in groups:
        attrs = {"enabled": "true", "mainActiveSkill": "1", "mainActiveSkillCalcs": "1"}
        if slot:
            attrs["slot"] = slot
        attrs.update(gattrs)
        a = " ".join(f'{k}="{escape(str(v))}"' for k, v in attrs.items())
        out.append(f"\t\t\t<Skill {a}>\n")
        for g in gems:
            ga = {"level": "20", "quality": "0", "enabled": "true", "enableGlobal1": "true", "enableGlobal2": "true"}
            ga.update(g)
            out.append("\t\t\t\t<Gem " + " ".join(f'{k}="{escape(str(v))}"' for k, v in ga.items()) + "/>\n")
        out.append("\t\t\t</Skill>\n")
    out.append("\t\t</SkillSet>\n\t</Skills>\n")
    out.append('\t<Tree activeSpec="1">\n\t\t<Spec treeVersion="3_29" classId="2" ascendClassId="0" secondaryAscendClassId="0" nodes=""/>\n\t</Tree>\n')
    out.append('\t<Items activeItemSet="1" useSecondWeaponSet="nil">\n')
    for i, text in items:
        out.append(item_xml(i, text))
    out.append('\t\t<ItemSet id="1" useSecondWeaponSet="nil">\n')
    for name, i in slots:
        out.append(f'\t\t\t<Slot name="{name}" itemId="{i}"/>\n')
    out.append("\t\t</ItemSet>\n\t</Items>\n")
    out.append('\t<Config>\n\t\t<Input name="enemyIsBoss" string="None"/>\n\t</Config>\n')
    out.append("</PathOfBuilding>\n")
    return "".join(out)


def main():
    dump = find_dump()
    skills = json.load(open(os.path.join(dump, "skills.json")))
    gems = json.load(open(os.path.join(dump, "gems.json")))
    # skill id -> (gem id, effect index) for the gem whose effect list holds it (first effect preferred)
    effect_of = {}
    for gid, g in sorted(gems.items()):
        for i, r in enumerate(g.get("grantedEffectList", [])):
            sid = r.get("$ref", "").replace("data.skills.", "")
            if sid not in effect_of or i + 1 < effect_of[sid][1]:
                effect_of[sid] = (gid, i + 1)
    outdir = os.path.join(HERE, "builds")
    os.makedirs(outdir, exist_ok=True)
    for f in os.listdir(outdir):
        if f.endswith(".xml"):
            os.remove(os.path.join(outdir, f))
    n = 0

    def write(name, xml):
        nonlocal n
        with open(os.path.join(outdir, name + ".xml"), "w") as fh:
            fh.write(xml)
        n += 1

    for sid in HOOK_SKILLS:
        s = skills[sid]
        nparts = len(s.get("parts") or []) or 1
        wt = s.get("weaponTypes") or {}
        items, slots = [], []
        for key, base in WEAPONS:
            if key in wt:
                items.append((1, f"Rarity: NORMAL\n{base}"))
                slots.append(("Weapon 1", 1))
                break
        if sid in effect_of:
            gid, idx = effect_of[sid]
            gem = gem_attrs(gems, gid)
        else:
            # hidden, item-granted skill: a gem naming only the skill id (SkillsTab.lua:487-492)
            gem, idx = {"skillId": sid}, 1
        for part in range(1, nparts + 1):
            g = dict(gem)
            if len(s.get("parts") or []) > 0:
                g["skillPart"] = str(part)
                g["skillPartCalcs"] = str(part)
            groups = [("Weapon 1" if slots else None, [g], {"mainActiveSkill": str(idx), "mainActiveSkillCalcs": str(idx)})]
            write(f"{sid}_p{part}", build_xml(groups, items, slots))
            if sid in ("Cyclone", "CycloneAltX", "VaalCyclone") and part == 1:
                # unarmed branch of initialFunc
                write(f"{sid}_unarmed", build_xml([(None, [g], {"mainActiveSkill": str(idx), "mainActiveSkillCalcs": str(idx)})], [], []))

    # EnemyExplode: an explode source on an amulet; the "Explode" socket group is the only one.
    amulet = (
        "Rarity: RARE\nSK-B Explode\nOnyx Amulet\n"
        "Enemies you Kill have a 20% chance to Explode, dealing a tenth of their maximum Life as Physical Damage\n"
        "Enemies you Kill have a 30% chance to Explode, dealing a quarter of their maximum Life as Fire Damage"
    )
    # (CalcSetup.lua:1535-1542 builds its gem without a skill part, so only part 1 runs)
    write("EnemyExplode", build_xml([], [(1, amulet)], [("Amulet", 1)]))
    # Raise Zombie: the minion's GAZombieCorpseGroundImpact (3rd skill) and, with the
    # "minions explode" mod, MinionInstability (an extra minion skill, 4th).
    ring = (
        "Rarity: RARE\nSK-B Instability\nIron Ring\n"
        "Minions explode when reduced to Low Life, dealing 33% of their maximum Life as Fire Damage to surrounding Enemies"
    )
    for name, minion_skill, its, sl in (
        ("GAZombieCorpseGroundImpact", 3, [], []),
        ("MinionInstability", 4, [(1, ring)], [("Ring 1", 1)]),
    ):
        g = dict(gem_attrs(gems, "Metadata/Items/Gems/SkillGemRaiseZombie"),
                 skillMinionSkill=str(minion_skill), skillMinionSkillCalcs=str(minion_skill))
        write(name, build_xml([(None, [g], {})], its, sl))
    # Avenging Flame: granted by the Flamewood support to the totem-summoning skill it supports.
    groups = [(None, [
        gem_attrs(gems, "Metadata/Items/Gems/SkillGemFireball"),
        gem_attrs(gems, "Metadata/Items/Gems/SkillGemSupportFlamewood"),
    ], {"mainActiveSkill": "2", "mainActiveSkillCalcs": "2"})]
    write("AvengingFlame", build_xml(groups, [], []))
    # AzmeriHydraBarrage: the Imperfect Hydra spectre's 2nd skill (Spectres.lua:4259-4279).
    hydra = "Metadata/Monsters/LeagueAzmeri/SpecialCorpses/Hydra/HydraLow"
    g = dict(gem_attrs(gems, "Metadata/Items/Gems/SkillGemRaiseSpectre"),
             skillMinion=hydra, skillMinionCalcs=hydra, skillMinionSkill="2", skillMinionSkillCalcs="2")
    write("AzmeriHydraBarrage", build_xml([(None, [g], {})], [], [], spectres=[hydra]))
    print(f"{n} builds -> {outdir}")


if __name__ == "__main__":
    main()
