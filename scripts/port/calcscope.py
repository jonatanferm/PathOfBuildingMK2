"""The calc port's in-scope Lua line ranges (shared by provenance/globals_lint/pairs_inventory).

`docs/port/scope.toml` (owned by U0) is authoritative once it exists. Until then, and as
the seed for it, DEFAULT_SCOPE is derived from the "Lua sources" column of the calc doc
§11.1 unit table at the pin. Skill hook closures in Data/Skills/*.lua are in scope only
for their own line ranges, which come from the dump's `closures.json`.

scope.toml format:
    pin = "<40-hex sha>"
    [[file]]
    path = "Modules/CalcOffence.lua"     # relative to PathOfBuilding/src ("../runtime/lua/x.lua" allowed)
    ranges = ["1-6263"]                  # inclusive; "*" = whole file
    owner = "OF-*"                       # optional, informational
"""

import json
import os

try:
    import tomllib
except ImportError:  # Python < 3.11
    tomllib = None

# (path relative to src/, [ranges], owners) — calc doc §11.1 "Lua sources" column.
DEFAULT_SCOPE = [
    ("Modules/Common.lua", [(306, 306), (477, 560), (709, 800), (897, 962), (1013, 1023)], "P2, CT"),
    ("Modules/ModTools.lua", [(1, 258)], "CT"),
    ("Modules/CalcTools.lua", [(1, 303)], "CT"),
    ("Modules/CalcBreakdown.lua", [(1, 255)], "CT"),
    ("Modules/CalcBase.lua", [(1, 93)], "CT"),
    ("Modules/CalcFormat.lua", [(1, 74)], "CT"),
    ("Modules/Data.lua", [(51, 93), (583, 600), (1033, 1077)], "CT, PT"),
    ("Modules/ModParser.lua", [(1, 7021)], "MP0-MP7"),
    ("Modules/DataLegionLookUpTableHelper.lua", [(1, 262)], "LUT"),
    ("Modules/DataAbyssJewelLookUpTableHelper.lua", [(1, 275)], "LUT"),
    ("Modules/DataJewelFileLoader.lua", [(1, 95)], "LUT"),
    ("Classes/PassiveTree.lua", [(1, 908), (1041, 1062)], "PT"),
    ("Classes/Item.lua", [(1, 2736)], "IT-A, IT-B, IT-C"),
    ("Modules/ItemSlotHelper.lua", [(1, 40)], "IT-B"),
    ("Modules/Utils.lua", [(1, 125)], "IT-B"),
    ("Modules/ItemTools.lua", [(1, 413)], "IT-C"),
    ("Classes/SkillsTab.lua", [(441, 630), (1129, 1274), (1478, 1502)], "BM-B"),
    ("Classes/PartyTab.lua", [(516, 669), (718, 1042)], "BM-B"),
    ("Classes/ConfigTab.lua", [(839, 1129), (1130, 1204)], "BM-B, CO-C"),
    ("Classes/ModStore.lua", [(1, 971)], "MS-A, MS-B"),
    ("Classes/ModDB.lua", [(1, 370)], "MS-A"),
    ("Classes/ModList.lua", [(1, 249)], "MS-A"),
    ("Classes/PassiveSpec.lua", [(1, 2494)], "PS-A, PS-B, PS-C"),
    ("Modules/Build.lua", [(976, 1099), (2222, 2335)], "BM-A"),
    ("Classes/ItemsTab.lua", [(1216, 1727), (2998, 3010)], "BM-A"),
    ("Classes/TreeTab.lua", [(500, 560)], "BM-A"),
    ("Classes/CalcsTab.lua", [(163, 225), (441, 773)], "BM-A, PWR"),
    ("../runtime/lua/xml.lua", [(1, 186)], "BM-A"),
    ("Modules/ConfigOptions.lua", [(1, 2380)], "CO-A, CO-B"),
    ("Data/ModMap.lua", [(1, 441)], "CO-C"),
    ("Modules/CalcActiveSkill.lua", [(1, 934)], "AS"),
    ("Modules/CalcSetup.lua", [(1, 1975)], "CS0-CS3"),
    ("Modules/PantheonTools.lua", [(1, 18)], "CS1"),
    ("Modules/CalcPerform.lua", [(1, 3990)], "PF-*"),
    ("Modules/CalcDefence.lua", [(1, 3873)], "DF-*"),
    ("Modules/CalcOffence.lua", [(1, 6263)], "OF-*"),
    ("Modules/CalcTriggers.lua", [(1, 1627)], "TR-A, TR-B"),
    ("Modules/CalcMirages.lua", [(1, 427)], "MI"),
    ("Modules/Calcs.lua", [(1, 872)], "CA"),
]

SKILL_FILES = {
    "Data/Skills/act_int.lua": "SK-A",
    "Data/Skills/act_dex.lua": "SK-B",
    "Data/Skills/act_str.lua": "SK-B",
    "Data/Skills/other.lua": "SK-B",
    "Data/Skills/minion.lua": "SK-B",
    "Data/Skills/spectre.lua": "SK-B",
    "Data/Skills/sup_str.lua": "SK-B",
}


def _parse_range(r):
    if isinstance(r, str):
        if r.strip() == "*":
            return (1, 10 ** 9)
        a, _, b = r.partition("-")
        return (int(a), int(b or a))
    a, b = r
    return (int(a), int(b))


def skill_closure_ranges(closures_json):
    """{path: [(a, b), ...]} for every skill-hook closure in the dump's closures.json."""
    out = {}
    if not closures_json or not os.path.exists(closures_json):
        return out
    with open(closures_json) as fh:
        data = json.load(fh)
    for c in data.get("closures", []):
        if not c.get("family", "").startswith("skill-hook:"):
            continue
        path, _, rng = c["src"].rpartition(":")
        a, _, b = rng.partition("-")
        out.setdefault(path, []).append((int(a), int(b)))
    for v in out.values():
        v.sort()
    return out


class Scope:
    def __init__(self, entries, source):
        self.entries = entries  # list of (path, [(a, b)], owner)
        self.source = source
        self._lines = {}
        for path, ranges, _ in entries:
            self._lines.setdefault(path, []).extend(ranges)

    def files(self):
        return list(self._lines)

    def ranges(self, path):
        return sorted(self._lines.get(path, []))

    def contains(self, path, line):
        for a, b in self._lines.get(path, ()):
            if a <= line <= b:
                return True
        return False

    def to_toml(self, pin):
        out = ["# In-scope Lua line ranges for the calc port (calc doc §9.6). Owned by U0.",
               "# Draft generated by scripts/port/provenance.py scope --emit.",
               'pin = "%s"' % pin, ""]
        for path, ranges, owner in self.entries:
            out.append("[[file]]")
            out.append('path = "%s"' % path)
            out.append("ranges = [%s]" % ", ".join('"%d-%d"' % r for r in ranges))
            if owner:
                out.append('owner = "%s"' % owner)
            out.append("")
        return "\n".join(out)


def default_scope(closures_json=None):
    entries = [(p, list(r), o) for p, r, o in DEFAULT_SCOPE]
    sk = skill_closure_ranges(closures_json)
    for path, owner in SKILL_FILES.items():
        if path in sk:
            entries.append((path, sk[path], owner))
    return Scope(entries, "default (calc doc §11.1%s)" % (" + closures.json" if sk else ""))


def load_scope(path=None, closures_json=None, required=False):
    """Load scope.toml if it exists (or `path` is given), else the default scope."""
    if path and os.path.exists(path):
        if tomllib is None:
            raise SystemExit("reading %s needs Python >= 3.11 (tomllib)" % path)
        with open(path, "rb") as fh:
            doc = tomllib.load(fh)
        entries = []
        for f in doc.get("file", []):
            entries.append((f["path"], [_parse_range(r) for r in f.get("ranges", ["*"])], f.get("owner", "")))
        return Scope(entries, path)
    if required:
        raise SystemExit("scope file %s not found" % path)
    return default_scope(closures_json)


def default_closures_json(main_root):
    return os.path.join(main_root, "data", "dump", "closures.json")
