#!/usr/bin/env python3
"""Derive a slice5 build (calc doc §9.3: one weapon, no auras; a single attack or spell) from an
all57 build: every socket group of the active skill set except the build's main socket group
(`<Build mainSocketGroup>`) gets `enabled="false"`. Nothing else changes (items, tree, config).

CCR-0012: no all57 build meets the slice5 definition as is, so U0 derives all five. The source
build must already have one weapon (Weapon 2 empty or a shield), no aura/curse/link/minion
`Extra*` mod from its tree or items, and a main skill that is a plain attack or spell.
`--check` verifies a derived build against the oracle's own view of it (`pob_oracle all
<build.xml> --out-dir DIR` writes DIR/stages.json): exactly one active skill and no such mod.

Usage:
  derive_slice5.py <parity>/builds/<src>.xml <out>.xml
  derive_slice5.py --check DIR/stages.json [...]
"""
import json
import re
import sys


def derive(src: str) -> str:
    lines = src.split("\n")
    main = int(re.search(r'<Build [^>]*\bmainSocketGroup="(\d+)"', src).group(1))
    active_set = re.search(r'<Skills [^>]*\bactiveSkillSet="(\d+)"', src)
    active_set = active_set.group(1) if active_set else None
    in_active = active_set is None
    idx = 0
    out = []
    for line in lines:
        m = re.search(r'<SkillSet [^>]*\bid="(\d+)"', line)
        if m:
            in_active = m.group(1) == active_set
            idx = 0
        if re.search(r"</SkillSet>", line):
            in_active = active_set is None
        if in_active and re.search(r"<Skill\s", line):
            idx += 1
            if idx != main:
                line, n = re.subn(r'(<Skill\s[^>]*?\benabled=")true(")', r"\1false\2", line)
        out.append(line)
    if idx < main:
        raise SystemExit(f"main socket group {main} not found ({idx} groups)")
    return "\n".join(out)


BAD_MODS = {"ExtraAura", "ExtraCurse", "ExtraAuraDebuff", "ExtraLinkEffect", "ExtraMinion"}


def check(stages_path: str) -> int:
    s = json.load(open(stages_path))
    skills = [o["id"] for o in s.get("skillOutputs") or [] if o.get("enabled") is not False]
    bad = sorted({f"{m['name']}@{m.get('source', '')}" for m in s["initEnv"]["player"]["modDB"]["mods"] if m["name"] in BAD_MODS})
    print(f"{stages_path}: active skills {skills}; aura/curse/link/minion Extra* mods {bad}")
    return 0 if len(skills) == 1 and not bad else 1


if __name__ == "__main__":
    if sys.argv[1] == "--check":
        sys.exit(max(check(p) for p in sys.argv[2:]))
    src, dst = sys.argv[1], sys.argv[2]
    text = open(src, encoding="utf-8").read()
    open(dst, "w", encoding="utf-8").write(derive(text))
