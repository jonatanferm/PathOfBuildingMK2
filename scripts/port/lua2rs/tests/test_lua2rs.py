"""lua2rs tests (unit T1).

    python3 -m unittest discover -s scripts/port/lua2rs/tests

Golden tests translate the miniature ModParser.lua / ConfigOptions.lua under fixtures/pob and
compare with golden/*.rs (regenerate with LUA2RS_UPDATE_GOLDEN=1 and review the diff).
The real-PoB tests run against an intact checkout at the pin (POB_DIR, the main checkout's
.PathOfBuilding, or its data/pob copy) and assert the T1 acceptance gates; they are skipped
when none is found. LUA2RS_CARGO=1 also runs the `cargo check` of the generated code.
"""

import json
import os
import sys
import unittest

HERE = os.path.dirname(os.path.abspath(__file__))
LUA2RS = os.path.dirname(HERE)
PORT = os.path.dirname(LUA2RS)
if PORT not in sys.path:
    sys.path.insert(0, PORT)

from lua2rs import cli, configopts, lpat, modparser  # noqa: E402
from lua2rs.common import LuaSource, Untranslatable, extract_comments, rust_f64, rust_str  # noqa: E402
from lua2rs.shape import load_shape  # noqa: E402
import pobpaths  # noqa: E402

FIXTURE_POB = os.path.join(HERE, "fixtures", "pob")
GOLDEN = os.path.join(HERE, "golden")
UPDATE = os.environ.get("LUA2RS_UPDATE_GOLDEN") == "1"


def real_pob():
    """An intact PathOfBuilding checkout at the pin, or None."""
    cands = []
    if os.environ.get("POB_DIR"):
        cands.append(os.environ["POB_DIR"])
    try:
        cands.append(pobpaths.find_pob(None))
    except SystemExit:
        pass
    cands.append(os.path.join(pobpaths.main_checkout(), "data", "pob"))
    for c in cands:
        p = os.path.join(c, "src", "Modules", "ModParser.lua")
        try:
            with open(p, "rb") as fh:
                if fh.read().count(b"\n") >= 7000:
                    return c
        except OSError:
            pass
    return None


REAL = real_pob()


def golden(test, name, text):
    path = os.path.join(GOLDEN, name)
    if UPDATE or not os.path.exists(path):
        with open(path, "w", encoding="utf-8") as fh:
            fh.write(text)
        if not UPDATE:
            test.fail("golden %s did not exist; written, review it" % name)
        return
    with open(path, encoding="utf-8") as fh:
        want = fh.read()
    test.maxDiff = None
    test.assertEqual(want, text, "golden %s differs (LUA2RS_UPDATE_GOLDEN=1 to accept)" % name)


class TestPatterns(unittest.TestCase):
    def test_capture_counts(self):
        self.assertEqual(lpat.capture_count("^(%d+)%% increased"), 1)
        self.assertEqual(lpat.capture_count("(%d+) to (%d+) added (%a+) damage"), 3)
        self.assertEqual(lpat.capture_count("%(not a capture%)"), 0)
        self.assertEqual(lpat.capture_count("[(]x[)]"), 0)
        self.assertEqual(lpat.capture_count("a ((%d+) b)"), 2)
        self.assertEqual(lpat.captures("()x"), ["pos"])

    def test_first_capture_digits(self):
        self.assertEqual(lpat.first_capture_digits("per (%d+) rage"), "num")
        self.assertEqual(lpat.first_capture_digits("([%+%-]?%d+)%% chance"), "num")
        self.assertEqual(lpat.first_capture_digits("if (%a+) charges"), "str")
        self.assertEqual(lpat.first_capture_digits("([%a%s]+) has"), "str")
        self.assertEqual(lpat.first_capture_digits("(%D+) x"), "str")
        self.assertEqual(lpat.first_capture_digits("per (.+) jewel"), "mixed")
        self.assertEqual(lpat.first_capture_digits("with (%w+) exposure"), "mixed")
        self.assertEqual(lpat.first_capture_digits("up to %+([%d%.]+)%%"), "mixed")   # "." alone has no digit
        self.assertEqual(lpat.first_capture_digits("no captures"), "none")


class TestLiterals(unittest.TestCase):
    def test_numbers(self):
        self.assertEqual(rust_f64(100), "100.0")
        self.assertEqual(rust_f64(0.5), "0.5")
        self.assertEqual(rust_f64(-16), "-16.0")
        self.assertEqual(rust_f64(1e-05), "1.0e-05")
        self.assertEqual(rust_f64(float("inf")), "f64::INFINITY")

    def test_strings(self):
        self.assertEqual(rust_str('a"b\\c\n'), '"a\\"b\\\\c\\n"')
        # luaast yields latin-1-decoded bytes; UTF-8 sequences round-trip
        self.assertEqual(rust_str("Mj\xc3\xb6lner"), '"Mjölner"')
        with self.assertRaises(Untranslatable):
            rust_str("\xff")

    def test_comments(self):
        cm = extract_comments([b'x = "--not" -- yes', b"-- whole", b"y = [[ -- no ]] -- after", b"z = 1"])
        self.assertEqual(cm, {1: "yes", 2: "whole", 3: "after"})


class TestGolden(unittest.TestCase):
    def setUp(self):
        self.shape = load_shape()
        self.flags = cli.load_flag_names()

    def test_modparser_lists(self):
        src = LuaSource(FIXTURE_POB, "Modules/ModParser.lua")
        for name in modparser.LISTS:
            g = modparser.ListGen(src, self.shape, self.flags)
            text, entries = g.generate(name)
            golden(self, "%s.rs" % name, text)

    def test_modparser_range(self):
        src = LuaSource(FIXTURE_POB, "Modules/ModParser.lua")
        st, t, entries = modparser.list_entries(src, "specialModList")
        a, b = entries[2].line, entries[5].line
        g = modparser.ListGen(src, self.shape, self.flags)
        text, sel = g.generate("specialModList", (a, b), "push_special_test")
        self.assertEqual([e.line for e in sel], [e.line for e in entries[2:6]])
        golden(self, "specialModList_range.rs", text)

    def test_config(self):
        src = LuaSource(FIXTURE_POB, "Modules/ConfigOptions.lua")
        g = configopts.ConfigGen(src, self.shape, self.flags)
        text, inline, shared = g.generate()
        golden(self, "config_apply.rs", text)
        self.assertEqual([a.id for a in inline],
                         ["ConfigOptions[%d].apply" % i for i in (2, 3, 4, 5, 6, 7, 8)])
        self.assertEqual([a.ok for a in inline], [True, True, True, True, True, True, False])
        self.assertEqual(len(shared), 1)

    def test_shape_override(self):
        shape = load_shape(overrides={"modtype": "ModType::{t}", "mod_fn": "createMod"})
        src = LuaSource(FIXTURE_POB, "Modules/ModParser.lua")
        text, _ = modparser.ListGen(src, shape, self.flags).generate("specialModList")
        self.assertIn('createMod(n!("EnergyShield"), ModType::MORE, -100.0, None, 0, 0, &[])', text)
        self.assertNotIn("r#mod(", text)

    def test_fixture_stats(self):
        src = LuaSource(FIXTURE_POB, "Modules/ModParser.lua")
        _, entries = modparser.ListGen(src, self.shape, self.flags).generate("specialModList")
        st = modparser.stats(entries)
        self.assertEqual(st["all"], {"total": 21, "ok": 20})
        self.assertEqual(st["fail_reasons"], {"vararg-closure": 1})
        _, entries = modparser.ListGen(src, self.shape, self.flags).generate("modTagList")
        st = modparser.stats(entries)
        self.assertEqual(st["duplicates"], [entries[6].line])
        # `mod` is declared below modTagList, so inside it the name is a (nil) global
        self.assertEqual(st["fail_reasons"], {"modtag-param1-num-or-string": 1, "call-target": 1})


@unittest.skipIf(REAL is None, "no intact PathOfBuilding checkout at the pin")
class TestRealPoB(unittest.TestCase):
    """The T1 acceptance gates (calc doc §11.1) on the pinned sources."""

    @classmethod
    def setUpClass(cls):
        cls.rep = cli.coverage(REAL, load_shape())

    def test_gate_lists_translate(self):
        for name in ("formList", "modNameList", "modFlagList"):
            a = self.rep["modparser"][name]["all"]
            self.assertEqual(a["ok"], a["total"], name)

    def test_gate_special(self):
        so = self.rep["modparser"]["specialModList"]["static+oneline"]
        self.assertGreaterEqual(so["ok"] / so["total"], 0.95)
        self.assertEqual(self.rep["modparser"]["specialModList"]["all"]["total"], 2099)

    def test_gate_config(self):
        c = self.rep["config"]["inline"]
        self.assertEqual(c["total"], 531)
        self.assertGreaterEqual(c["ok"] / c["total"], 0.80)

    def test_counts(self):
        mp = self.rep["modparser"]
        self.assertEqual(mp["formList"]["all"]["total"], 85)
        self.assertEqual(mp["modNameList"]["all"]["total"], 724)
        self.assertEqual(mp["modFlagList"]["all"]["total"], 181)
        self.assertEqual(mp["preFlagList"]["all"]["total"], 190)
        self.assertEqual(mp["modTagList"]["all"]["total"], 665)       # 666 fields, 1 duplicate key
        self.assertEqual(mp["modTagList"]["duplicates"], [1853])
        closures = mp["modTagList"]["fn-oneline"]["total"] + mp["modTagList"]["fn-singleret"]["total"]
        closures += mp["preFlagList"]["fn-singleret"]["total"]
        self.assertEqual(closures, 143)                                  # doc §8 / MP2: 136 + 7

    def test_config_ids_and_hashes_match_dump(self):
        path = os.path.join(pobpaths.main_checkout(), "data", "dump", "closures.json")
        if not os.path.exists(path):
            self.skipTest("data/dump/closures.json not found")
        with open(path, encoding="utf-8") as fh:
            dump = json.load(fh)
        want = {c["id"]: (c["hash"], c["src"]) for c in dump["closures"] if c["family"] == "config:apply"}
        src = LuaSource(REAL, configopts.CONFIG)
        _, inline, shared = configopts.ConfigGen(src, load_shape()).generate()
        got = {a.id: (a.hash, "%s:%d-%d" % (configopts.CONFIG, a.line, a.endline)) for a in inline}
        for k, v in got.items():
            self.assertEqual(want.get(k), v, k)
        # the one id not inline is the shared mapAffixDropDownFunction
        self.assertEqual(sorted(set(want) - set(got)), ["ConfigOptions[278].apply"])

    def test_provenance_of_output(self):
        import provenance
        src = LuaSource(REAL, modparser.MODPARSER)
        text, _ = modparser.ListGen(src, load_shape()).generate("modTagList")
        n = 0
        for line in text.splitlines():
            m = provenance.RE_POB.search(line)
            if m:
                n += 1
                a, b = int(m.group("a")), int(m.group("b"))
                self.assertEqual(m.group("h"), provenance.range_hash(REAL, m.group("path"), a, b))
        self.assertGreater(n, 10)

    @unittest.skipUnless(os.environ.get("LUA2RS_CARGO") == "1", "set LUA2RS_CARGO=1 to cargo check")
    def test_generated_code_compiles(self):
        self.assertEqual(cli.run_check(REAL, None), 0)


if __name__ == "__main__":
    unittest.main()
