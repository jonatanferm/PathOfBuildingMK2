#!/usr/bin/env python3
"""Derive the OF-D unit corpus (calc doc §9.3 `unit/<id>`) from all57 builds.

Each entry takes one `.parity/builds/<stem>.xml`, makes socket group <g> the
main socket group (`Build mainSocketGroup`) and its active skill <k> the main
active skill in both MAIN and CALCS mode, optionally replacing one gem, and
writes `<stem>_g<g>[k<k>][_<tag>].xml` next to this script. The entries pick
the paths of CalcOffence.lua 2174-2605 the all57 main skills do not reach:
brands (hitTimeOverride, AttachedBrandCount), warcries, cooldown skills,
triggered skills (triggerTime / triggerRate), channelled skills
(hitTimeMultiplier), dual wielding (bothWeaponAttack), totems, repeats and
trauma (Boneshatter).

Usage: derive.py [--parity DIR]   (default: the enclosing checkout's .parity)
"""
import argparse
import pathlib
import re

HERE = pathlib.Path(__file__).resolve().parent

# (stem, group, active skill index or None, (old gem nameSpec, new gem attrs) or None, tag)
ENTRIES = [
    ("upstream_OccVortex", 1, None, None, ""),  # Flame Dash: cooldown, travel
    ("upstream_OccVortex", 2, None, None, ""),  # Storm Brand: brand, hitTimeOverride
    ("upstream_OccVortex", 3, None, None, ""),  # Enduring Cry: warcry + cooldown
    ("upstream_OccVortex", 3, 2, None, ""),  # Frost Bomb: cooldown spell
    ("upstream_OccVortex", 4, None, None, ""),  # Shield Charge: attack
    (
        "upstream_OccVortex",
        4,
        None,
        (
            "Shield Charge",
            'gemId="Metadata/Items/Gems/SkillGemBoneshatter" skillId="Boneshatter" nameSpec="Boneshatter"',
        ),
        "boneshatter",
    ),  # trauma
    ("upstream_Dual_Wield_Cospris_CoC", 5, 1, None, ""),  # Cyclone, dual wield, channel
    ("upstream_Dual_Wield_Cospris_CoC", 5, 2, None, ""),  # Freezing Pulse via CoC: triggered
    ("upstream_Dual_Wield_Cospris_CoC", 2, None, None, ""),  # Dash
    ("upstream_Generals_Perforate_Zerker", 3, 1, None, ""),  # Blade Flurry: hitTimeMultiplier
    ("upstream_Generals_Perforate_Zerker", 7, None, None, ""),  # CwC Cyclone
    ("upstream_Dual_Savior", 1, None, None, ""),  # Double Strike, dual wield
    ("allflame_exp_041", 2, None, None, ""),  # Sigil of Power
    ("allflame_exp_041", 8, None, None, ""),  # Arcanist Brand
    ("upstream_Generals_Perforate_Zerker", 6, None, None, ""),  # Vaal Ancestral Warchief: totem
    ("upstream_Mirage_Archer_Toxic_Rain", 2, None, None, ""),  # Ballista Totem + Toxic Rain
    (
        "upstream_Mirage_Archer_Toxic_Rain",
        1,
        None,
        (
            "Toxic Rain",
            'gemId="Metadata/Items/Gems/SkillGemScourgeArrow" skillId="ScourgeArrow" nameSpec="Scourge Arrow"',
        ),
        "scourgearrow",
    ),  # hitTimeMultiplier (set by its preDamageFunc)
    (
        "upstream_Dual_Savior",
        2,
        None,
        (
            "Dual Strike",
            'gemId="Metadata/Items/Gems/SkillGemChargedDash" skillId="ChargedDash" nameSpec="Charged Dash"',
        ),
        "chargeddash",
    ),  # channelled, hitTimeMultiplier
]


def find_parity() -> pathlib.Path:
    d = HERE
    while d != d.parent:
        if (d / ".parity" / "builds").is_dir():
            return d / ".parity"
        d = d.parent
    raise SystemExit("no .parity/builds found (pass --parity)")


def derive(text: str, group: int, active, swap) -> str:
    text = re.sub(r'mainSocketGroup="\d+"', f'mainSocketGroup="{group}"', text, count=1)
    # the <Skill> elements of the (first) skill set, in order
    skills = list(re.finditer(r"<Skill\b[^>]*>(.*?)</Skill>", text, flags=re.S))
    if group > len(skills):
        raise SystemExit(f"group {group} > {len(skills)} socket groups")
    m = skills[group - 1]
    block = m.group(0)
    head_end = block.index(">") + 1
    head, body = block[:head_end], block[head_end:]
    head = re.sub(r'\benabled="false"', 'enabled="true"', head)
    if active is not None:
        head = re.sub(r'mainActiveSkill="\d+"', f'mainActiveSkill="{active}"', head)
        head = re.sub(r'mainActiveSkillCalcs="\d+"', f'mainActiveSkillCalcs="{active}"', head)
    if swap is not None:
        old, new = swap
        pat = re.compile(r'<Gem\b[^>]*nameSpec="' + re.escape(old) + r'"[^>]*/>')
        g = pat.search(body)
        if not g:
            raise SystemExit(f"gem {old} not in group {group}")
        gem = g.group(0)
        gem = re.sub(r'\s(gemId|skillId|nameSpec)="[^"]*"', "", gem)
        gem = gem.replace("<Gem ", f"<Gem {new} ", 1)
        body = body[: g.start()] + gem + body[g.end() :]
    return text[: m.start()] + head + body + text[m.end() :]


def main() -> None:
    ap = argparse.ArgumentParser()
    ap.add_argument("--parity", type=pathlib.Path)
    a = ap.parse_args()
    parity = a.parity or find_parity()
    for stem, group, active, swap, tag in ENTRIES:
        src = (parity / "builds" / f"{stem}.xml").read_text(encoding="utf-8")
        name = f"{stem}_g{group}" + (f"k{active}" if active is not None else "") + (f"_{tag}" if tag else "")
        (HERE / f"{name}.xml").write_text(derive(src, group, active, swap), encoding="utf-8")
        print(name)


if __name__ == "__main__":
    main()
